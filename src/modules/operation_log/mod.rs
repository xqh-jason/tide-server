//! 操作日志域（codegen 生成）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 操作日志 CRUD 端点：`POST /api/v1/operation_log/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .push(Router::with_path("list").post(api::list_operation_logs))
        .push(Router::with_path("create").post(api::create_operation_log))
        .push(Router::with_path("update").post(api::update_operation_log))
        .push(Router::with_path("get").post(api::get_operation_log))
        .push(Router::with_path("delete").post(api::delete_operation_log))
}
