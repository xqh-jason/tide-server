//! 文件域：本地磁盘上传 + 元数据管理。
//!
//! 端点：`POST /api/v1/file/{list,upload,get,delete}` + `GET /api/v1/file/download`，
//! AuthRequired + OperationLog + ApiPermission 在 `infra/router.rs` 统一挂载。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 文件端点：函数顺序 = 路由挂载顺序 = `list → upload → get → download → delete`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["文件"])
        .push(Router::with_path("list").post(api::list_files))
        .push(Router::with_path("upload").post(api::upload_file))
        .push(Router::with_path("get").post(api::get_file))
        .push(Router::with_path("download").get(api::download_file))
        .push(Router::with_path("delete").post(api::delete_file))
}
