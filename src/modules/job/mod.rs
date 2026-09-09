//! 定时任务域：任务 CRUD 与调度器实时同步（执行日志见 job_log 域）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod scheduler;
pub mod service;
mod validate;

/// 定时任务端点：`POST /api/v1/job/{list,create,update,get,delete,update-status,run-once}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["定时任务"])
        .push(Router::with_path("list").post(api::list_jobs))
        .push(Router::with_path("create").post(api::create_job))
        .push(Router::with_path("update").post(api::update_job))
        .push(Router::with_path("get").post(api::get_job))
        .push(Router::with_path("delete").post(api::delete_job))
        .push(Router::with_path("update-status").post(api::update_job_status))
        .push(Router::with_path("run-once").post(api::run_job_once))
}
