//! 认证业务：登录（查用户 → 校验密码 → 取角色 → 签发 JWT）。

use sea_orm::DatabaseConnection;

use crate::infra::config::Jwt as JwtConfig;
use crate::modules::auth::dto::{LoginReq, LoginResp};
use crate::modules::user::repo as user_repo;
use crate::utils::error::AppError;
use crate::utils::{crypt, jwt};

pub async fn login(
    db: &DatabaseConnection,
    jwt_cfg: &JwtConfig,
    req: LoginReq,
) -> Result<LoginResp, AppError> {
    let user = user_repo::find_by_username(db, &req.username)
        .await?
        .ok_or_else(|| AppError::Biz("用户名或密码错误".into()))?;

    // 密码校验失败与用户不存在返回同一提示，避免账号枚举
    if !crypt::verify_password(&req.password, &user.password) {
        return Err(AppError::Biz("用户名或密码错误".into()));
    }

    let roles = user_repo::find_roles_by_user_id(db, user.id).await?;
    let role_keys: Vec<String> = roles.into_iter().map(|r| r.role_key).collect();
    let token = jwt::sign(
        user.id,
        &user.username,
        &role_keys,
        &jwt_cfg.secret,
        jwt_cfg.ttl_seconds,
    )?;

    Ok(LoginResp { token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_role, sys_user, sys_user_role};
    use crate::utils::crypt;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set,
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
    async fn seed_user(
        db: &sea_orm::DatabaseConnection,
    ) -> (u64, u64, String, String) {
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
        sys_role::Entity::delete_by_id(role_id).exec(db).await.unwrap();
        sys_user::Entity::delete_by_id(user_id).exec(db).await.unwrap();
    }

    #[tokio::test]
    async fn login_success_returns_token_with_roles() {
        let db = test_db().await;
        let (uid, rid, username, role_key) = seed_user(&db).await;

        let resp = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username,
                password: "pass123".into(),
            },
        )
        .await
        .unwrap();

        let claims = crate::utils::jwt::verify(&resp.token, "test-secret").unwrap();
        assert_eq!(claims.user_id, uid);
        assert_eq!(claims.roles, vec![role_key]);
        cleanup(&db, uid, rid).await;
    }

    #[tokio::test]
    async fn login_wrong_password_returns_biz_error() {
        let db = test_db().await;
        let (uid, rid, username, _) = seed_user(&db).await;

        let err = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username,
                password: "bad".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Biz(_)));
        cleanup(&db, uid, rid).await;
    }

    #[tokio::test]
    async fn login_unknown_user_returns_biz_error() {
        let db = test_db().await;
        let err = login(
            &db,
            &test_jwt_cfg(),
            LoginReq {
                username: "no_such_user_xyz".into(),
                password: "x".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Biz(_)));
    }
}
