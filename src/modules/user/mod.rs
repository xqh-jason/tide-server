//! 用户域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 router.rs 挂到 /api/v1 下。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

pub fn routes() -> Router {
    Router::new()
        .push(Router::with_path("list").post(api::list_users))
        .push(Router::with_path("by-username").post(api::get_by_username))
        .push(Router::with_path("info").post(api::info))
        .push(Router::with_path("access-codes").post(api::access_codes))
}
