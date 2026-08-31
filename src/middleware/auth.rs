//! 认证中间件（W2 第 4 步）：`Authorization: Bearer <token>` → JWT 校验 → 黑名单检查。
//!
//! 校验通过后把 `AuthUser` 按类型写入 Depot，handler 用 `depot.get_typed::<AuthUser>()`
//! 取当前登录用户；鉴权失败返回 HTTP 401 + `{code:0, data, message}` 契约体
//! （唯一使用 HTTP 状态码的例外：token 缺失/无效/黑名单/用户失效，
//!  前端 vben 的 authenticateResponseInterceptor 按 401 触发登出/刷新）。
//! 白名单（login/health）不挂本中间件，由路由组装层控制。

use salvo::prelude::*;
use sea_orm::{DatabaseConnection, EntityTrait};

use crate::entity::sys_user;
use crate::infra::state::AppState;
use crate::utils::error::AppError;
use crate::utils::response::ApiResponse;

/// 当前登录用户（JWT 载荷的视图，认证中间件注入）。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: u64,
    pub username: String,
    pub roles: Vec<String>,
}

impl AuthUser {
    /// 从请求上下文（Depot）读取当前登录用户（`AuthRequired` 写入）。
    pub fn from_depot(depot: &Depot) -> Result<Self, AppError> {
        depot
            .get_typed::<AuthUser>()
            .map_err(|_| AppError::Biz("unauthorized".into()))
            .cloned()
    }
}

/// 校验用户当前有效：存在、未软删除、启用。
///
/// 认证中间件在 JWT 验证通过后调用，避免 token 仍有效但用户已被
/// 禁用/删除的请求继续进入业务层（用户有效性统一在认证层过滤）。
pub(crate) async fn ensure_user_active(
    db: &DatabaseConnection,
    user_id: u64,
) -> anyhow::Result<bool> {
    let Some(user) = sys_user::Entity::find_by_id(user_id).one(db).await? else {
        return Ok(false);
    };
    Ok(user.deleted_at.is_none() && user.status == 1)
}

/// 从请求头提取 Bearer token。
pub(crate) fn bearer_token(req: &Request) -> Option<String> {
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

        // 黑名单优先：登出后的 token 直接拒绝（缓存键统一带前缀，避免与验证码等冲突）
        let blacklist_key = format!("jwt:blacklist:{token}");
        if state.cache.exists(&blacklist_key) {
            unauthorized(res, ctrl);
            return;
        }

        match crate::utils::jwt::verify(&token, &state.config.jwt.secret) {
            Ok(claims) => match ensure_user_active(&state.db, claims.user_id).await {
                Ok(true) => {
                    depot.insert_typed(AuthUser {
                        user_id: claims.user_id,
                        username: claims.username,
                        roles: claims.roles,
                    });
                }
                _ => {
                    tracing::debug!("user inactive or unavailable, user_id={}", claims.user_id);
                    unauthorized(res, ctrl);
                }
            },
            Err(err) => {
                tracing::debug!("jwt verify failed: {err}");
                unauthorized(res, ctrl);
            }
        }
    }
}

/// 渲染统一 401 响应（HTTP 401 + code 0）并跳过后续 handler。
fn unauthorized(res: &mut Response, ctrl: &mut FlowCtrl) {
    res.status_code(StatusCode::UNAUTHORIZED);
    res.render(Json(ApiResponse::<()>::fail("unauthorized")));
    ctrl.skip_rest();
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, Database};
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

    #[tokio::test]
    async fn ensure_user_active_accepts_active_user() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;

        let result = ensure_user_active(&db, user.id).await;

        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn ensure_user_active_rejects_deleted_user() {
        let db = test_db().await;
        let user = seed_user(&db, 1, Some(chrono::Utc::now().naive_utc())).await;

        let result = ensure_user_active(&db, user.id).await;

        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(!result.unwrap(), "软删除用户不应通过有效性检查");
    }

    #[tokio::test]
    async fn ensure_user_active_rejects_disabled_user() {
        let db = test_db().await;
        let user = seed_user(&db, 0, None).await;

        let result = ensure_user_active(&db, user.id).await;

        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(!result.unwrap(), "禁用用户不应通过有效性检查");
    }

    #[tokio::test]
    async fn ensure_user_active_rejects_missing_user() {
        let db = test_db().await;

        let result = ensure_user_active(&db, u64::MAX).await;

        assert!(!result.unwrap(), "不存在的用户不应通过有效性检查");
    }
}
