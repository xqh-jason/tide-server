//! 认证域：登录。JWT 校验在认证中间件（含黑名单检查），接口级授权在 middleware/api_permission。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

use crate::middleware::{auth::AuthRequired, op_log::OperationLog};

pub mod api;
pub mod dto;
pub mod service;

/// 认证端点：`POST /api/v1/auth/login`（公开）、`POST /api/v1/auth/refresh`
/// （公开，凭 HttpOnly Cookie 鉴权）与 `POST /api/v1/auth/logout`（需登录态）。
pub fn routes() -> Router {
    Router::with_path("auth")
        .oapi_tags(["认证"])
        .push(Router::with_path("login").post(api::login))
        // 刷新：公开端点（凭 HttpOnly Cookie 中的 refresh token 鉴权，spec §4.3）
        .push(Router::with_path("refresh").post(api::refresh))
        // logout 需要登录态：子路由单独挂认证中间件
        .push(
            Router::with_path("logout")
                .hoop(AuthRequired)
                .hoop(OperationLog)
                .post(api::logout),
        )
}
