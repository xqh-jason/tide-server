//! 认证中间件（W2 第 4 步）：`Authorization: Bearer <token>` → JWT 校验 → 黑名单检查。
//!
//! 校验通过后把 `AuthUser` 按类型写入 Depot，handler 用 `depot.get_typed::<AuthUser>()`
//! 取当前登录用户；失败统一返回 HTTP 401 + `{code:401, data, message}` 契约体。
//! 白名单（login/health）不挂本中间件，由路由组装层控制。

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::utils::response::ApiResponse;

/// 当前登录用户（JWT 载荷的视图，认证中间件注入）。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: u64,
    pub username: String,
    pub roles: Vec<String>,
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
            Ok(claims) => {
                depot.insert_typed(AuthUser {
                    user_id: claims.user_id,
                    username: claims.username,
                    roles: claims.roles,
                });
            }
            Err(err) => {
                tracing::debug!("jwt verify failed: {err}");
                unauthorized(res, ctrl);
            }
        }
    }
}

/// 渲染统一 401 响应并跳过后续 handler。
fn unauthorized(res: &mut Response, ctrl: &mut FlowCtrl) {
    res.status_code(StatusCode::UNAUTHORIZED);
    res.render(Json(ApiResponse::<()>::fail(401, "unauthorized")));
    ctrl.skip_rest();
}
