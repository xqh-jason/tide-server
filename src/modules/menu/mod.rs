//! 菜单域：菜单树 + vben schema 转换 + 按钮权限码（W3 起承载菜单管理 CRUD）。
//!
//! 注意：契约端点路径仍是 `POST /api/v1/user/menus`，由 router.rs 把本域路由
//! 挂在 `/user` 组下，这里只返回路径片段。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

pub fn routes() -> Router {
    Router::with_path("menus").post(api::menus)
}
