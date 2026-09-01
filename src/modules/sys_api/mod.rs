//! API 权限域（对应 sys_api 表）：W3 实现权限点登记 CRUD + `sys_role_api` 关联维护。
//!
//! 注意：`sys_api` 仅作为权限点登记数据，W3 第一版主授权链路仍走按钮权限码；
//! 接口级集中授权（path/method → 角色映射）留待后续引入。

use salvo::Router;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

pub fn routes() -> Router {
    Router::new()
        .push(Router::with_path("list").post(api::list_apis))
        .push(Router::with_path("create").post(api::create_api))
        .push(Router::with_path("update").post(api::update_api))
        .push(Router::with_path("get").post(api::get_api))
        .push(Router::with_path("delete").post(api::delete_api))
}
