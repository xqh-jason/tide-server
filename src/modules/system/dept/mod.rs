//! 部门域：部门树 CRUD + 组织归属。
//!
//! 端点（受保护，`/api/v1/dept/*`）：`POST {list,create,update,get,delete}`；
//! `list` 返回树（不分页），负责人列表按 `sys_user_dept.is_leader` 拼装。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

/// 部门端点：`POST /api/v1/dept/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["部门"])
        .push(Router::with_path("list").post(api::list_depts))
        .push(Router::with_path("create").post(api::create_dept))
        .push(Router::with_path("update").post(api::update_dept))
        .push(Router::with_path("get").post(api::get_dept))
        .push(Router::with_path("delete").post(api::delete_dept))
}
