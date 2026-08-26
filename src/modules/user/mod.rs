//! 用户域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 router.rs 挂到 /api/v1 下。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

pub fn routes() -> Router {
    Router::with_path("user")
        .push(Router::with_path("list").get(api::list_users))
        .push(Router::with_path("by-username/{username}").get(api::get_by_username))
}
