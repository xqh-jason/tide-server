//! 定时任务执行日志域（codegen 生成后裁剪：只读 + 删除，无 create / update）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 执行日志端点：`POST /api/v1/job-log/{list,get,delete,delete-batch}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["定时任务日志"])
        .push(Router::with_path("list").post(api::list_job_logs))
        .push(Router::with_path("get").post(api::get_job_log))
        .push(Router::with_path("delete").post(api::delete_job_log))
        .push(Router::with_path("delete-batch").post(api::delete_job_log_batch))
}
