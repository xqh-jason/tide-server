//! 操作日志域（codegen 生成后裁剪：只读 + 删除，无 create / update）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 操作日志端点：`POST /api/v1/operation-log/{list,get,delete,delete-batch}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["操作日志"])
        .push(Router::with_path("list").post(api::list_operation_logs))
        .push(Router::with_path("get").post(api::get_operation_log))
        .push(Router::with_path("delete").post(api::delete_operation_log))
        .push(Router::with_path("delete-batch").post(api::delete_operation_log_batch))
}
