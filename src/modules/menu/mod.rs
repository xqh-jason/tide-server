//! 菜单域：vben 菜单树契约 + 菜单管理 CRUD + 按钮权限码同源。
//!
//! 本域同时承载两处端点：
//! - `user_routes()`：`POST /api/v1/user/menus`（vben 动态路由契约，挂在 user 组下）
//! - `routes()`：`POST /api/v1/menu/{list,create,update,get,delete}`（管理端点，挂在 menu 组下）

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 菜单管理 CRUD 端点：`POST /api/v1/menu/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .push(Router::with_path("list").post(api::list_menus))
        .push(Router::with_path("create").post(api::create_menu))
        .push(Router::with_path("update").post(api::update_menu))
        .push(Router::with_path("get").post(api::get_menu))
        .push(Router::with_path("delete").post(api::delete_menu))
}

/// vben 菜单树契约端点：`POST /api/v1/user/menus`（由 router.rs 挂在 user 组下）。
pub fn user_routes() -> Router {
    Router::with_path("menus").post(api::menus)
}
