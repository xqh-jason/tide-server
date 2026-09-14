//! 刷新凭证域：`sys_refresh_token` 的管理面（在线会话列表 / 强制下线）。
//!
//! 会话的创建与吊销主体在 auth 流程（登录 / 登出 / 刷新），本域只承担
//! 管理端查询与强制下线；refresh token 校验原语在 repo 供认证链路复用。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 刷新凭证端点：`POST /api/v1/refresh-token/{list,delete,delete-batch,force-logout,force-logout-user}`。
/// 后两者是特殊契约端点：前者按会话 id（单条），后者按用户 id（该用户全部有效会话）。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["刷新凭证"])
        .push(Router::with_path("list").post(api::list_refresh_tokens))
        .push(Router::with_path("delete").post(api::delete_refresh_token))
        .push(Router::with_path("delete-batch").post(api::delete_refresh_token_batch))
        .push(Router::with_path("force-logout").post(api::force_logout_refresh_token))
        .push(Router::with_path("force-logout-user").post(api::force_logout_user))
}
