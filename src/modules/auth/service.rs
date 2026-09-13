//! 认证业务：登录（验证码 → 查用户 → 校验密码 → 取角色 → 创建凭证 → 签发 JWT）与刷新。

use sea_orm::ActiveValue::Set;
use sea_orm::ConnectionTrait;

use crate::entity::sys_login_log;
use crate::infra::config::Jwt as JwtConfig;
use crate::modules::auth::dto::{LoginMeta, LoginReq, LoginResp};
use crate::modules::refresh_token::dto::CreateRefreshTokenParams;
use crate::modules::refresh_token::repo as refresh_token_repo;
use crate::modules::refresh_token::service as refresh_token_service;
use crate::modules::user::repo as user_repo;
use crate::utils::cache::Cache;
use crate::utils::error::AppError;
use crate::utils::{crypt, jwt};

/// 登录：验证码 → 查用户 → 校验密码与状态 → 取角色 → 创建凭证记录 → 签发 JWT，
/// 全程落登录日志。用户不存在 / 密码错误 / 已禁用对外统一返回「用户名或密码错误」，
/// 防账号枚举；验证码失败单独提示（不泄露账号信息），原因分级进日志。
///
/// 成功返回 `(响应体, refresh_token 明文)`：明文只出现一次，由 handler 设进
/// HttpOnly Cookie（不进响应体）（spec §4.3）。
pub async fn login(
    db: &impl ConnectionTrait,
    jwt_cfg: &JwtConfig,
    cache: &dyn Cache,
    req: LoginReq,
    meta: LoginMeta,
) -> Result<(LoginResp, String), AppError> {
    // 先验码后查库：注定失败的登录不浪费一次 DB 查询；
    // 验证码一次性消费发生在 captcha::service 内部（无论成败）。
    if let Err(err) =
        crate::modules::captcha::service::verify_captcha(cache, &req.captcha_id, &req.captcha_value)
    {
        // 失败原因下探到登录日志分级，对外透传 captcha 层的原始文案
        let msg = match &err {
            AppError::Biz(m) if m.contains("已过期") => "验证码已过期",
            _ => "验证码错误",
        };
        record_login(db, &req, 0, 0, msg, &meta).await;
        return Err(err);
    }

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

    // 创建凭证记录（uuid 明文只回传一次给 handler 种 Cookie），以新记录 id 签发 JWT
    let (record, refresh_token) = match refresh_token_service::create_refresh_token(
        db,
        CreateRefreshTokenParams {
            user_id: user.id,
            username: user.username.clone(),
            ip: meta.ip.clone(),
            agent: meta.agent.clone(),
            refresh_ttl_seconds: jwt_cfg.refresh_ttl_seconds,
        },
    )
    .await
    {
        Ok(ok) => ok,
        Err(err) => {
            record_login(db, &req, user.id, 0, "凭证创建失败", &meta).await;
            return Err(err);
        }
    };

    match jwt::sign(
        user.id,
        &user.username,
        &role_keys,
        record.id,
        &jwt_cfg.secret,
        jwt_cfg.ttl_seconds,
    ) {
        Ok(token) => {
            record_login(db, &req, user.id, 1, "登录成功", &meta).await;
            Ok((LoginResp { token }, refresh_token))
        }
        Err(err) => {
            record_login(db, &req, user.id, 0, "token 签发失败", &meta).await;
            Err(err.into())
        }
    }
}

/// 刷新 access token：按 refresh token 哈希查可用会话（未吊销且未过期），
/// 确认用户仍有效后重查**最新角色**签发新 access token（同一 refresh_token_id），
/// 并回写 `last_active_at`。会话不存在/已吊销/已过期/用户失效统一报错，
/// 由 handler 转成真 HTTP 401（spec §4.3）。
pub async fn refresh(
    db: &impl ConnectionTrait,
    jwt_cfg: &JwtConfig,
    refresh_token: &str,
) -> Result<String, AppError> {
    let hash = crypt::sha256_hex(refresh_token);
    let Some((record, user)) = refresh_token_repo::find_usable_with_user_by_hash(db, &hash).await?
    else {
        return Err(AppError::Biz("登录已过期，请重新登录".into()));
    };

    // 用户有效性：禁用/软删即拒绝（防「禁用用户借存活凭证复活」）
    let user_active = user
        .as_ref()
        .map(|u| u.deleted_at.is_none() && u.status == 1)
        .unwrap_or(false);
    if !user_active {
        return Err(AppError::Biz("登录已过期，请重新登录".into()));
    }

    // 角色从 DB 重查：权限变更最迟随凭证刷新生效，不沿用旧 token 快照
    let roles = user_repo::find_roles_by_user_id(db, record.user_id).await?;
    let role_keys: Vec<String> = roles.into_iter().map(|r| r.role_key).collect();

    let token = jwt::sign(
        record.user_id,
        &record.username,
        &role_keys,
        record.id,
        &jwt_cfg.secret,
        jwt_cfg.ttl_seconds,
    )?;

    // 刷新本身算活跃
    refresh_token_repo::touch_last_active_at(db, record.id).await?;
    Ok(token)
}

/// 登录日志落库：失败只降级，不影响登录结果。
async fn record_login(
    db: &impl ConnectionTrait,
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
    use crate::entity::{sys_login_log, sys_refresh_token, sys_role, sys_user, sys_user_role};
    use crate::utils::cache::MemoryCache;
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
            refresh_ttl_seconds: 604_800,
        }
    }

    /// 造唯一用户 + 关联角色（唯一命名避免并行冲突），返回 (user_id, role_id, username, role_key)。
    async fn seed_user(db: &impl ConnectionTrait) -> (u64, u64, String, String) {
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

    fn test_meta() -> LoginMeta {
        LoginMeta {
            ip: "127.0.0.1".into(),
            agent: "test-agent".into(),
        }
    }

    /// 造一个带有效验证码的登录请求：generate 后从 cache 直读答案（生成接口不回传答案）。
    fn login_req_with_captcha(cache: &MemoryCache, username: &str, password: &str) -> LoginReq {
        let resp = crate::modules::captcha::service::generate_captcha(cache).unwrap();
        let answer = cache
            .get(&format!("captcha:{}", resp.captcha_id))
            .expect("验证码答案应已入 cache");
        LoginReq {
            username: username.to_string(),
            password: password.to_string(),
            captcha_id: resp.captcha_id,
            captcha_value: answer,
        }
    }

    async fn latest_login_log(db: &impl ConnectionTrait, username: &str) -> sys_login_log::Model {
        sys_login_log::Entity::find()
            .filter(sys_login_log::Column::Username.eq(username))
            .order_by_desc(sys_login_log::Column::Id)
            .one(db)
            .await
            .unwrap()
            .expect("登录日志应存在")
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 造指定状态的会话行（直连 ActiveModel），返回 (会话, 明文)。
    async fn seed_record(
        db: &impl ConnectionTrait,
        user_id: u64,
        revoked: bool,
        expired: bool,
    ) -> (sys_refresh_token::Model, String) {
        let plaintext = unique_name("sess_plain");
        let plaintext = format!("{plaintext:0<32}");
        let now = chrono::Local::now().naive_local();
        let model = sys_refresh_token::ActiveModel {
            user_id: Set(user_id),
            username: Set("auth-svc".to_string()),
            refresh_token_hash: Set(crypt::sha256_hex(&plaintext)),
            ip: Set("127.0.0.1".to_string()),
            agent: Set("svc-agent".to_string()),
            last_active_at: Set(now),
            expires_at: Set(if expired {
                now - chrono::Duration::seconds(60)
            } else {
                now + chrono::Duration::days(7)
            }),
            revoked_at: Set(revoked.then_some(now)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        (model, plaintext)
    }

    // ===== 会话化登录 / 刷新（spec §4.3）=====

    #[tokio::test]
    async fn login_creates_refresh_token_record_and_returns_plaintext() {
        let db = test_txn().await;
        let (uid, _rid, username, _role_key) = seed_user(&db).await;
        let cache = MemoryCache::new();

        let (resp, refresh_token) = login(
            &db,
            &test_jwt_cfg(),
            &cache,
            login_req_with_captcha(&cache, &username, "pass123"),
            test_meta(),
        )
        .await
        .unwrap();

        // 明文 refresh token 只在返回值中出现一次（32 位 hex），供 handler 种 Cookie
        assert_eq!(refresh_token.len(), 32, "uuid simple 明文应为 32 位 hex");
        assert!(refresh_token.chars().all(|c| c.is_ascii_hexdigit()));

        // 落库为明文哈希；JWT claims.refresh_token_id 指向该会话
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let record = sys_refresh_token::Entity::find()
            .filter(sys_refresh_token::Column::UserId.eq(uid))
            .one(&db)
            .await
            .unwrap()
            .expect("登录成功应创建凭证记录");
        assert_eq!(
            record.refresh_token_hash,
            crypt::sha256_hex(&refresh_token),
            "落库哈希应为明文的 SHA-256"
        );
        let claims = jwt::verify(&resp.token, "test-secret").unwrap();
        assert_eq!(claims.refresh_token_id, record.id, "JWT 应携带会话 id");
    }

    #[tokio::test]
    async fn refresh_returns_token_with_same_grant_and_fresh_roles() {
        let db = test_txn().await;
        let (uid, _rid, _username, role_key) = seed_user(&db).await;
        let (record, plaintext) = seed_record(&db, uid, false, false).await;

        let token = refresh(&db, &test_jwt_cfg(), &plaintext).await.unwrap();

        let claims = jwt::verify(&token, "test-secret").unwrap();
        assert_eq!(claims.user_id, uid);
        assert_eq!(
            claims.refresh_token_id, record.id,
            "刷新后的 token 应绑定同一会话"
        );
        assert_eq!(
            claims.roles,
            vec![role_key],
            "角色应从 DB 重查而非沿用旧 token"
        );

        // 刷新本身算活跃：last_active_at 应被回写
        let after = sys_refresh_token::Entity::find_by_id(record.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let now = chrono::Local::now().naive_local();
        assert!(
            (now - after.last_active_at) < chrono::Duration::minutes(1),
            "刷新应回写 last_active_at"
        );
    }

    #[tokio::test]
    async fn refresh_rejects_revoked_expired_and_unknown_token() {
        let db = test_txn().await;
        let (uid, _rid, _username, _role_key) = seed_user(&db).await;

        let (revoked, revoked_plain) = seed_record(&db, uid, true, false).await;
        let (_expired, expired_plain) = seed_record(&db, uid, false, true).await;

        for (label, plaintext) in [
            ("已吊销", revoked_plain.as_str()),
            ("已过期", expired_plain.as_str()),
            ("不存在", "ffffffffffffffffffffffffffffffff"),
        ] {
            let err = refresh(&db, &test_jwt_cfg(), plaintext).await;
            assert!(err.is_err(), "{label} 的 refresh token 应被拒绝");
        }
        let _ = revoked;
    }

    #[tokio::test]
    async fn refresh_rejects_inactive_user_even_with_usable_record() {
        let db = test_txn().await;
        // 禁用用户 + 完全可用的会话：刷新仍应拒绝
        let username = unique_name("login_refresh_disabled");
        let user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set(crypt::hash_password("pass123").unwrap()),
            nickname: Set("刷新禁用测试".to_string()),
            status: Set(0),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let (_session, plaintext) = seed_record(&db, user.id, false, false).await;

        let err = refresh(&db, &test_jwt_cfg(), &plaintext).await;

        assert!(
            err.is_err(),
            "会话有效但用户被禁用，刷新应被拒绝（防解封复活）"
        );
    }
    #[tokio::test]
    async fn login_success_returns_token_with_roles() {
        let db = test_txn().await;
        let (uid, _rid, username, role_key) = seed_user(&db).await;
        let cache = MemoryCache::new();

        let (resp, _refresh_token) = login(
            &db,
            &test_jwt_cfg(),
            &cache,
            login_req_with_captcha(&cache, &username, "pass123"),
            test_meta(),
        )
        .await
        .unwrap();

        let claims = crate::utils::jwt::verify(&resp.token, "test-secret").unwrap();
        let log = latest_login_log(&db, &username).await;

        assert_eq!(claims.user_id, uid);
        assert_eq!(claims.roles, vec![role_key]);
        assert_eq!(log.status, 1, "成功登录应记录 status=1");
        assert_eq!(log.user_id, uid, "成功登录应记录用户 id");
        assert_eq!(log.msg, "登录成功");
    }

    #[tokio::test]
    async fn login_wrong_password_returns_biz_error() {
        let db = test_txn().await;
        let (uid, _rid, username, _) = seed_user(&db).await;
        let cache = MemoryCache::new();

        let err = login(
            &db,
            &test_jwt_cfg(),
            &cache,
            login_req_with_captcha(&cache, &username, "bad"),
            test_meta(),
        )
        .await
        .unwrap_err();
        let log = latest_login_log(&db, &username).await;

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
        let db = test_txn().await;
        let username = unique_name("login_unknown");
        let cache = MemoryCache::new();
        let err = login(
            &db,
            &test_jwt_cfg(),
            &cache,
            login_req_with_captcha(&cache, &username, "x"),
            test_meta(),
        )
        .await
        .unwrap_err();
        let log = latest_login_log(&db, &username).await;

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
        let db = test_txn().await;
        let username = unique_name("login_disabled");
        let cache = MemoryCache::new();
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
            &cache,
            login_req_with_captcha(&cache, &username, "pass123"),
            test_meta(),
        )
        .await
        .unwrap_err();

        let log = latest_login_log(&db, &username).await;
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

    /// 验证码答案错误：对外明确提示（不并入「用户名或密码错误」），且码被消费。
    #[tokio::test]
    async fn login_with_wrong_captcha_returns_biz_error() {
        let db = test_txn().await;
        let username = unique_name("login_bad_captcha");
        let cache = MemoryCache::new();
        let resp = crate::modules::captcha::service::generate_captcha(&cache).unwrap();

        let err = login(
            &db,
            &test_jwt_cfg(),
            &cache,
            LoginReq {
                username: username.clone(),
                password: "pass123".into(),
                captcha_id: resp.captcha_id,
                captcha_value: "0000".into(),
            },
            test_meta(),
        )
        .await
        .unwrap_err();
        let log = latest_login_log(&db, &username).await;

        assert!(
            matches!(&err, AppError::Biz(message) if message.contains("验证码错误")),
            "验证码错误应单独提示，实际：{err:?}"
        );
        assert_eq!(log.status, 0);
        assert_eq!(log.user_id, 0, "验证码失败发生在查库之前，user_id 为 0");
        assert_eq!(log.msg, "验证码错误", "日志内部分级记录验证码错误");
    }

    /// 验证码 id 不存在（未生成 / 已过期）：提示刷新，日志记录过期。
    #[tokio::test]
    async fn login_with_unknown_captcha_id_reports_expired() {
        let db = test_txn().await;
        let username = unique_name("login_expired_captcha");
        let cache = MemoryCache::new();

        let err = login(
            &db,
            &test_jwt_cfg(),
            &cache,
            LoginReq {
                username: username.clone(),
                password: "pass123".into(),
                captcha_id: "ghost-id".into(),
                captcha_value: "1234".into(),
            },
            test_meta(),
        )
        .await
        .unwrap_err();
        let log = latest_login_log(&db, &username).await;

        assert!(
            matches!(&err, AppError::Biz(message) if message.contains("已过期")),
            "未知验证码 id 应提示过期刷新，实际：{err:?}"
        );
        assert_eq!(log.msg, "验证码已过期", "日志内部分级记录验证码过期");
    }
}
