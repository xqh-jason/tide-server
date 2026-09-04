//! 认证域：登录（W2）。JWT 校验在黑名单/中间件，权限校验在 middleware/permission（W2 尾）。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

use crate::middleware::{auth::AuthRequired, op_log::OperationLog};

pub mod api;
pub mod dto;
pub mod service;

/// 认证端点：`POST /api/v1/auth/login`（公开）与 `POST /api/v1/auth/logout`（需登录态）。
pub fn routes() -> Router {
    Router::with_path("auth")
        .oapi_tags(["认证"])
        .push(Router::with_path("login").post(api::login))
        // logout 需要登录态：子路由单独挂认证中间件
        .push(
            Router::with_path("logout")
                .hoop(AuthRequired)
                .hoop(OperationLog)
                .post(api::logout),
        )
}
