//! 认证中间件：`Authorization: Bearer <token>` → JWT 校验 → 会话有效性合并查询。
//!
//! 校验通过后把 `AuthUser` 按类型写入 Depot，handler 用 `depot.get_typed::<AuthUser>()`
//! 取当前登录用户；鉴权失败返回 HTTP 401 + `{code:0, data, message}` 契约体
//! （唯一使用 HTTP 状态码的例外：token 缺失/无效/凭证已吊销或过期/用户失效，
//!  前端 vben 的 authenticateResponseInterceptor 按 401 触发登出/刷新）。
//! 白名单（login/refresh/health）不挂本中间件，由路由组装层控制。
//!
//! 会话状态在 `sys_refresh_token`（spec：2026-09-13-auth-refresh-session-design.md）：
//! 每请求一条合并 SQL（凭证未吊销未过期 JOIN 用户有效），登出/强制下线即时生效；
//! 黑名单已退役，cache 只承担 last_active_at 的 60s 节流键（丢失无害）。

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::system::refresh_token::repo as refresh_token_repo;
use crate::utils::error::AppError;
use crate::utils::response::ApiResponse;

/// 当前登录用户（JWT 载荷的视图，认证中间件注入）。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: u64,
    pub roles: Vec<String>,
    /// 所属登录会话 id（sys_refresh_token.id；登出/强制下线吊销时定位用）
    pub refresh_token_id: u64,
}

impl AuthUser {
    /// 从请求上下文（Depot）读取当前登录用户（`AuthRequired` 写入）。
    pub fn from_depot(depot: &Depot) -> Result<Self, AppError> {
        depot
            .get_typed::<AuthUser>()
            // 受保护路由必过 AuthRequired，缺失属于内部故障而非业务校验失败
            .map_err(|_| AppError::Internal(anyhow::anyhow!("auth user not found")))
            .cloned()
    }
}

/// 从请求头提取 Bearer token。
fn bearer_token(req: &Request) -> Option<String> {
    let header = req.headers().get("authorization")?.to_str().ok()?;
    header.strip_prefix("Bearer ").map(|s| s.trim().to_string())
}

/// 认证中间件：挂在受保护路由组上。
pub struct AuthRequired;

#[async_trait]
impl Handler for AuthRequired {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        let state = depot.get_typed::<AppState>().ok();
        let Some(token) = bearer_token(req) else {
            unauthorized(res, ctrl);
            return;
        };

        let Some(state) = state else {
            tracing::error!("app state not injected before auth middleware");
            unauthorized(res, ctrl);
            return;
        };

        match crate::utils::jwt::verify(&token, &state.config.jwt.secret) {
            Ok(claims) => {
                // 单条合并查询：凭证未吊销未过期 JOIN 用户行（启用/软删判定在下方）
                match refresh_token_repo::find_usable_with_user_by_id(
                    &state.db,
                    claims.refresh_token_id,
                )
                .await
                {
                    Ok(Some((record, user))) => {
                        let user_active = user
                            .map(|u| u.deleted_at.is_none() && u.status == 1)
                            .unwrap_or(false);
                        if !user_active {
                            tracing::debug!(
                                "user inactive or unavailable, user_id={}",
                                claims.user_id
                            );
                            unauthorized(res, ctrl);
                            return;
                        }
                        touch_last_active_throttled(state, record.id).await;
                        depot.insert_typed(AuthUser {
                            user_id: claims.user_id,
                            roles: claims.roles,
                            refresh_token_id: record.id,
                        });
                    }
                    Ok(None) => {
                        tracing::debug!(
                            "refresh token record unusable or missing, id={}",
                            claims.refresh_token_id
                        );
                        unauthorized(res, ctrl);
                    }
                    Err(err) => {
                        tracing::error!(%err, "refresh token query failed");
                        unauthorized(res, ctrl);
                    }
                }
            }
            Err(err) => {
                tracing::debug!("jwt verify failed: {err}");
                unauthorized(res, ctrl);
            }
        }
    }
}

/// 回写最后活跃时间：cache 键 60s 节流（每会话每分钟最多一次 UPDATE）。
/// 节流键丢失无害——最多多写一次；写库失败只记日志，不影响请求。
async fn touch_last_active_throttled(state: &AppState, refresh_token_id: u64) {
    let key = format!("refresh_token:touch:{refresh_token_id}");
    if state.cache.exists(&key) {
        return;
    }
    state
        .cache
        .set(&key, "1".into(), std::time::Duration::from_secs(60));
    if let Err(err) = refresh_token_repo::touch_last_active_at(&state.db, refresh_token_id).await {
        tracing::error!(%err, "last_active_at write failed");
    }
}

/// 渲染统一 401 响应（HTTP 401 + code 0）并跳过后续 handler。
fn unauthorized(res: &mut Response, ctrl: &mut FlowCtrl) {
    res.status_code(StatusCode::UNAUTHORIZED);
    res.render(Json(ApiResponse::<()>::fail("未登录或登录已过期")));
    ctrl.skip_rest();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_user;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, Database, DatabaseConnection};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    async fn seed_user(
        db: &DatabaseConnection,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique("auth_user")),
            password: Set("x".to_string()),
            nickname: Set("认证测试用户".to_string()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    // ===== 会话校验链黑盒测试（spec §4.2）：真库 + 手工清理 =====
    // （原 ensure_user_active 单元测试随函数移除：软删/禁用/缺失用户的行为
    //   已由下方黑盒用例在合并查询链路上覆盖）

    use std::sync::Arc;

    use crate::entity::sys_refresh_token;
    use crate::utils::cache::MemoryCache;

    /// 构造 AppState（真库 + 内存 cache + 空转调度器）。
    async fn app_state(db: DatabaseConnection) -> AppState {
        let config = crate::infra::config::Config::load().unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        AppState::new(config, db, Arc::new(MemoryCache::new()), scheduler)
    }

    /// 造凭证记录：`usable=false` 且 `expired=false` 表示吊销；`expired=true` 表示过期。
    async fn seed_record(
        db: &DatabaseConnection,
        user_id: u64,
        usable: bool,
        expired: bool,
    ) -> sys_refresh_token::Model {
        let now = chrono::Local::now().naive_local();
        let revoked_at = match (usable, expired) {
            (false, false) => Some(now),
            _ => None,
        };
        sys_refresh_token::ActiveModel {
            user_id: Set(user_id),
            username: Set("auth-mw".to_string()),
            refresh_token_hash: Set(unique("mw-hash")),
            ip: Set("127.0.0.1".to_string()),
            agent: Set("mw-agent".to_string()),
            last_active_at: Set(now),
            expires_at: Set(if expired {
                now - chrono::Duration::seconds(60)
            } else {
                now + chrono::Duration::days(7)
            }),
            revoked_at: Set(revoked_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 黑盒执行 AuthRequired：注入 Bearer token，返回 (响应, Depot)。
    async fn run_auth(state: AppState, token: &str) -> (Response, Depot) {
        let mut req = Request::new();
        req.headers_mut().insert(
            salvo::http::header::AUTHORIZATION,
            salvo::http::header::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let mut depot = Depot::new();
        depot.insert_typed(state);
        let mut res = Response::new();
        let mut ctrl = FlowCtrl::new(vec![Arc::new(AuthRequired)]);
        ctrl.call_next(&mut req, &mut depot, &mut res).await;
        (res, depot)
    }

    /// 清理用户及其全部凭证记录。
    async fn cleanup_user(db: &DatabaseConnection, user_id: u64) {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        sys_refresh_token::Entity::delete_many()
            .filter(sys_refresh_token::Column::UserId.eq(user_id))
            .exec(db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(user_id)
            .exec(db)
            .await
            .unwrap();
    }

    fn secret() -> String {
        crate::infra::config::Config::load().unwrap().jwt.secret
    }

    #[tokio::test]
    async fn auth_required_accepts_valid_record_and_user() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let record = seed_record(&db, user.id, true, false).await;
        let token =
            crate::utils::jwt::sign(user.id, &user.username, &[], record.id, &secret(), 600)
                .unwrap();

        let (res, depot) = run_auth(app_state(db.clone()).await, &token).await;

        cleanup_user(&db, user.id).await;

        assert_eq!(res.status_code, None, "有效会话 + 有效用户不应被拦截");
        let auth = depot.get_typed::<AuthUser>().unwrap();
        assert_eq!(auth.user_id, user.id);
        assert_eq!(auth.refresh_token_id, record.id);
    }

    #[tokio::test]
    async fn auth_required_rejects_revoked_record() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let record = seed_record(&db, user.id, false, false).await;
        let token =
            crate::utils::jwt::sign(user.id, &user.username, &[], record.id, &secret(), 600)
                .unwrap();

        let (res, _depot) = run_auth(app_state(db.clone()).await, &token).await;

        cleanup_user(&db, user.id).await;

        assert_eq!(
            res.status_code,
            Some(StatusCode::UNAUTHORIZED),
            "被吊销（登出/强制下线）的会话应立即 401"
        );
    }

    #[tokio::test]
    async fn auth_required_rejects_expired_record() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let record = seed_record(&db, user.id, true, true).await;
        let token =
            crate::utils::jwt::sign(user.id, &user.username, &[], record.id, &secret(), 600)
                .unwrap();

        let (res, _depot) = run_auth(app_state(db.clone()).await, &token).await;

        cleanup_user(&db, user.id).await;

        assert_eq!(
            res.status_code,
            Some(StatusCode::UNAUTHORIZED),
            "过期的会话应 401"
        );
    }

    #[tokio::test]
    async fn auth_required_rejects_token_without_refresh_token_id() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        // 旧版载荷（无 refresh_token_id 字段）：验签可通过，但必须因缺会话被拒
        let now = chrono::Utc::now().timestamp();
        let legacy = serde_json::json!({
            "user_id": user.id, "username": user.username, "roles": [],
            "iat": now, "exp": now + 600
        });
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &legacy,
            &jsonwebtoken::EncodingKey::from_secret(secret().as_bytes()),
        )
        .unwrap();

        let (res, _depot) = run_auth(app_state(db.clone()).await, &token).await;

        cleanup_user(&db, user.id).await;

        assert_eq!(
            res.status_code,
            Some(StatusCode::UNAUTHORIZED),
            "缺 refresh_token_id 的旧 token 应被拒绝（发版全员重登迁移路径）"
        );
    }

    #[tokio::test]
    async fn auth_required_rejects_disabled_user_with_usable_record() {
        let db = test_db().await;
        let user = seed_user(&db, 0, None).await;
        let record = seed_record(&db, user.id, true, false).await;
        let token =
            crate::utils::jwt::sign(user.id, &user.username, &[], record.id, &secret(), 600)
                .unwrap();

        let (res, _depot) = run_auth(app_state(db.clone()).await, &token).await;

        cleanup_user(&db, user.id).await;

        assert_eq!(
            res.status_code,
            Some(StatusCode::UNAUTHORIZED),
            "会话有效但用户被禁用仍应 401"
        );
    }

    #[tokio::test]
    async fn auth_required_rejects_soft_deleted_user_with_usable_record() {
        let db = test_db().await;
        let user = seed_user(&db, 1, Some(chrono::Local::now().naive_local())).await;
        let record = seed_record(&db, user.id, true, false).await;
        let token =
            crate::utils::jwt::sign(user.id, &user.username, &[], record.id, &secret(), 600)
                .unwrap();

        let (res, _depot) = run_auth(app_state(db.clone()).await, &token).await;

        cleanup_user(&db, user.id).await;

        assert_eq!(
            res.status_code,
            Some(StatusCode::UNAUTHORIZED),
            "会话有效但用户被软删仍应 401"
        );
    }
}
