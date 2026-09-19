//! 员工档案域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 `modules/mod.rs` 的 DOMAINS 登记表挂到 /api/v1/hr/employee 下。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

/// 员工档案端点：`POST /api/v1/hr/employee/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["员工档案"])
        .push(Router::with_path("list").post(api::list_employees))
        .push(Router::with_path("create").post(api::create_employee))
        .push(Router::with_path("update").post(api::update_employee))
        .push(Router::with_path("get").post(api::get_employee))
        .push(Router::with_path("delete").post(api::delete_employee))
}
