//! API 权限域（对应 sys_api 表）：权限点登记 CRUD + `sys_role_api` 关联维护。
//!
//! 授权链路为双通道：按钮权限码（`sys_menu.permission`，判定在 permission 域）+
//! 接口级集中授权（`path/method` → `sys_role_api` 角色映射，由
//! `middleware/api_permission.rs` 消费，未登记接口放行）。

use salvo::Router;
use salvo::oapi::RouterExt;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// API 权限点端点：`POST /api/v1/sys-api/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["接口权限"])
        .push(Router::with_path("list").post(api::list_apis))
        .push(Router::with_path("create").post(api::create_api))
        .push(Router::with_path("update").post(api::update_api))
        .push(Router::with_path("get").post(api::get_api))
        .push(Router::with_path("delete").post(api::delete_api))
}
