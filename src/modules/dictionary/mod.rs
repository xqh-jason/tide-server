//! 数据字典域（W5-3）：类型表与字典项表两级结构。
//!
//! 路由挂载见 `src/infra/router.rs`：`/api/v1/dictionary`（类型）与
//! `/api/v1/dictionary-detail`（字典项）两个路由组，均在 AuthRequired 之后。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 字典类型端点：`POST /api/v1/dictionary/{list,create,update,get,delete,get-by-type}`。
/// 顺序 = api.rs 函数顺序，特殊契约端点 `get-by-type` 排在 CRUD 之后。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["数据字典"])
        .push(Router::with_path("list").post(api::list_dictionaries))
        .push(Router::with_path("create").post(api::create_dictionary))
        .push(Router::with_path("update").post(api::update_dictionary))
        .push(Router::with_path("get").post(api::get_dictionary))
        .push(Router::with_path("delete").post(api::delete_dictionary))
        .push(Router::with_path("get-by-type").post(api::get_dictionary_by_type))
}

/// 字典项端点：`POST /api/v1/dictionary-detail/{list,create,update,get,delete}`。
pub fn detail_routes() -> Router {
    Router::new()
        .oapi_tags(["数据字典项"])
        .push(Router::with_path("list").post(api::list_dictionary_details))
        .push(Router::with_path("create").post(api::create_dictionary_detail))
        .push(Router::with_path("update").post(api::update_dictionary_detail))
        .push(Router::with_path("get").post(api::get_dictionary_detail))
        .push(Router::with_path("delete").post(api::delete_dictionary_detail))
}
