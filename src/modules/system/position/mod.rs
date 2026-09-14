//! 职位域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 `modules/mod.rs` 的 DOMAINS 登记表挂到 /api/v1/position 下。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

/// 职位端点：`POST /api/v1/position/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["职位"])
        .push(Router::with_path("list").post(api::list_positions))
        .push(Router::with_path("create").post(api::create_position))
        .push(Router::with_path("update").post(api::update_position))
        .push(Router::with_path("get").post(api::get_position))
        .push(Router::with_path("delete").post(api::delete_position))
}
