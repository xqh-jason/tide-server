//! 角色域：W3 第二步实现 CRUD + 分配菜单/API（事务）。
pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

use salvo::prelude::*;

pub fn routes() -> Router {
    Router::new()
        .push(Router::with_path("list").post(api::list_roles))
        .push(Router::with_path("create").post(api::create_role))
        .push(Router::with_path("update").post(api::update_role))
        .push(Router::with_path("get").post(api::get_role))
        .push(Router::with_path("delete").post(api::delete_role))
}
