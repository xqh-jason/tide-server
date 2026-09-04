//! 登录日志域（codegen 生成后裁剪：只读 + 删除，无 create / update）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 登录日志端点：`POST /api/v1/login-log/{list,get,delete,delete-batch}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["登录日志"])
        .push(Router::with_path("list").post(api::list_login_logs))
        .push(Router::with_path("get").post(api::get_login_log))
        .push(Router::with_path("delete").post(api::delete_login_log))
        .push(Router::with_path("delete-batch").post(api::delete_login_log_batch))
}
