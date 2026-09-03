//! 认证业务：登录（查用户 → 校验密码 → 取角色 → 签发 JWT）。

use sea_orm::ActiveValue::Set;
use sea_orm::DatabaseConnection;

use crate::entity::sys_login_log;
use crate::infra::config::Jwt as JwtConfig;
use crate::modules::auth::dto::{LoginMeta, LoginReq, LoginResp};
use crate::modules::user::repo as user_repo;
use crate::utils::error::AppError;
use crate::utils::{crypt, jwt};

pub async fn login(
    db: &DatabaseConnection,
    jwt_cfg: &JwtConfig,
    req: LoginReq,
    meta: LoginMeta,
) -> Result<LoginResp, AppError> {
    // 用户不存在：对外统一提示，日志内部分级
    let user = match user_repo::find_by_username(db, &req.username).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            record_login(db, &req, 0, 0, "用户不存在", &meta).await;
            return Err(AppError::Biz("用户名或密码错误".into()));
        }
        Err(err) => {
            record_login(db, &req, 0, 0, "查询用户失败", &meta).await;
            return Err(err.into());
        }
    };

    // 密码校验失败与用户不存在返回同一提示，避免账号枚举
    if !crypt::verify_password(&req.password, &user.password) {
        record_login(db, &req, user.id, 0, "密码错误", &meta).await;
        return Err(AppError::Biz("用户名或密码错误".into()));
    }
    // 禁用用户同样拒绝登录：对外提示保持一致，日志区分原因
    if user.status != 1 {
        record_login(db, &req, user.id, 0, "用户已被禁用", &meta).await;
        return Err(AppError::Biz("用户名或密码错误".into()));
    }

    let roles = user_repo::find_roles_by_user_id(db, user.id).await?;
    let role_keys: Vec<String> = roles.into_iter().map(|r| r.role_key).collect();
    match jwt::sign(
        user.id,
        &user.username,
        &role_keys,
        &jwt_cfg.secret,
        jwt_cfg.ttl_seconds,
    ) {
        Ok(token) => {
            record_login(db, &req, user.id, 1, "登录成功", &meta).await;
            Ok(LoginResp { token })
        }
        Err(err) => {
            record_login(db, &req, user.id, 0, "token 签发失败", &meta).await;
            Err(err.into())
        }
    }
}

/// 登录日志落库：失败只降级，不影响登录结果。
async fn record_login(
    db: &DatabaseConnection,
    req: &LoginReq,
    user_id: u64,
    status: i8,
    msg: &str,
    meta: &LoginMeta,
) {
    let model = sys_login_log::ActiveModel {
        user_id: Set(user_id),
        username: Set(truncate_chars(&req.username, 64)),
        ip: Set(truncate_chars(&meta.ip, 64)),
        agent: Set(truncate_chars(&meta.agent, 255)),
        status: Set(status),
        msg: Set(truncate_chars(msg, 255)),
        ..Default::default()
    };
    if let Err(err) = crate::modules::login_log::repo::create_login_log(db, model).await {
        tracing::error!(%err, "login log 落库失败");
    }
}

/// 按字符数截断，避免超长触发 DB 报错。
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_login_log, sys_role, sys_user, sys_user_role};
    use crate::utils::crypt;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, QueryOrder, Set,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一命名：pid + 原子序号，避免并行测试冲突。
    fn unique_name(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> sea_orm::DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    fn test_jwt_cfg() -> JwtConfig {
        JwtConfig {
            secret: "test-secret".into(),
            ttl_seconds: 7200,
        }
    }

    /// 造唯一用户 + 关联角色（唯一命名避免并行冲突），返回 (user_id, role_id, username, role_key)。
    async fn seed_user(db: &sea_orm::DatabaseConnection) -> (u64, u64, String, String) {
        let username = unique_name("login_test");
        let role_key = unique_name("super");

        let user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set(crypt::hash_password("pass123").unwrap()),
            nickname: Set("登录测试".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let role = sys_role::ActiveModel {
            role_name: Set("测试超管".to_string()),
            role_key: Set(role_key.clone()),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        sys_user_role::ActiveModel {
            user_id: Set(user.id),
            role_id: Set(role.id),
        }
        .insert(db)
        .await
        .unwrap();

        (user.id, role.id, username, role_key)
    }

    async fn cleanup(db: &sea_orm::DatabaseConnection, user_id: u64, role_id: u64) {
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(user_id))
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_by_id(role_id)
            .exec(db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(user_id)
            .exec(db)
            .await
            .unwrap();
    }

    fn test_meta() -> LoginMeta {
        LoginMeta {
            ip: "127.0.0.1".into(),
            agent: "test-agent".into(),
        }
    }

    async fn cleanup_login_logs(db: &sea_orm::DatabaseConnection, username: &str) {
        sys_login_log::Entity::delete_many()
            .filter(sys_login_log::Column::Username.eq(username))
            .exec(db)
            .await
            .unwrap();
    }

    async fn latest_login_log(
        db: &sea_orm::DatabaseConnection,
        username: &str,
    ) -> sys_login_log::Model {
        sys_login_log::Entity::find()
            .filter(sys_login_log::Column::Username.eq(username))
            .order_by_desc(sys_login_log::Column::Id)
            .one(db)
            .await
            .unwrap()
            .expect("登录日志应存在")
    }

    #[tokio::test]
    async fn login_success_returns_token_with_roles() {
        let db = test_db().await;
        let (uid, rid, username, role_key) = seed_user(&db).await;

        let resp = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username: username.clone(),
                password: "pass123".into(),
            },
            test_meta(),
        )
        .await
        .unwrap();

        let claims = crate::utils::jwt::verify(&resp.token, "test-secret").unwrap();
        let log = latest_login_log(&db, &username).await;
        cleanup_login_logs(&db, &username).await;
        cleanup(&db, uid, rid).await;

        assert_eq!(claims.user_id, uid);
        assert_eq!(claims.roles, vec![role_key]);
        assert_eq!(log.status, 1, "成功登录应记录 status=1");
        assert_eq!(log.user_id, uid, "成功登录应记录用户 id");
        assert_eq!(log.msg, "登录成功");
    }

    #[tokio::test]
    async fn login_wrong_password_returns_biz_error() {
        let db = test_db().await;
        let (uid, rid, username, _) = seed_user(&db).await;

        let err = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username: username.clone(),
                password: "bad".into(),
            },
            test_meta(),
        )
        .await
        .unwrap_err();
        let log = latest_login_log(&db, &username).await;
        cleanup_login_logs(&db, &username).await;
        cleanup(&db, uid, rid).await;

        assert!(
            matches!(&err, AppError::Biz(message) if message == "用户名或密码错误"),
            "密码错误对外应统一文案，实际：{err:?}"
        );
        assert_eq!(log.status, 0);
        assert_eq!(log.user_id, uid);
        assert_eq!(log.msg, "密码错误", "日志内部分级记录密码错误");
    }

    #[tokio::test]
    async fn login_unknown_user_returns_biz_error() {
        let db = test_db().await;
        let username = unique_name("login_unknown");
        let err = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username: username.clone(),
                password: "x".into(),
            },
            test_meta(),
        )
        .await
        .unwrap_err();
        let log = latest_login_log(&db, &username).await;
        cleanup_login_logs(&db, &username).await;

        assert!(
            matches!(&err, AppError::Biz(message) if message == "用户名或密码错误"),
            "未知用户对外应统一文案，实际：{err:?}"
        );
        assert_eq!(log.status, 0);
        assert_eq!(log.user_id, 0, "未知用户没有 user_id");
        assert_eq!(log.msg, "用户不存在", "日志内部分级记录用户不存在");
    }

    /// 禁用用户（status=0）即使密码正确也不允许登录，且与用户不存在返回同一提示（防枚举）。
    #[tokio::test]
    async fn login_disabled_user_returns_biz_error() {
        let db = test_db().await;
        let username = unique_name("login_disabled");
        let user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set(crypt::hash_password("pass123").unwrap()),
            nickname: Set("禁用登录测试".to_string()),
            status: Set(0),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let err = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username: username.clone(),
                password: "pass123".into(),
            },
            test_meta(),
        )
        .await
        .unwrap_err();

        let log = latest_login_log(&db, &username).await;
        cleanup_login_logs(&db, &username).await;
        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(
            matches!(&err, AppError::Biz(message) if message == "用户名或密码错误"),
            "禁用用户对外应统一文案，实际：{err:?}"
        );
        assert_eq!(log.status, 0);
        assert_eq!(log.user_id, user.id);
        assert_eq!(log.msg, "用户已被禁用", "日志内部分级记录禁用");
    }
}
