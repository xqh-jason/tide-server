//! 数据字典域（codegen 生成）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 数据字典 CRUD 端点：`POST /api/v1/dict/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .push(Router::with_path("list").post(api::list_dicts))
        .push(Router::with_path("create").post(api::create_dict))
        .push(Router::with_path("update").post(api::update_dict))
        .push(Router::with_path("get").post(api::get_dict))
        .push(Router::with_path("delete").post(api::delete_dict))
}
