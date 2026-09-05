//! 系统配置域：键值参数（/config）与网站设置（/site-config），W5-6。

use salvo::prelude::*;

use crate::middleware::auth::AuthRequired;
use crate::middleware::op_log::OperationLog;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 参数配置端点：`POST /api/v1/config/{list,create,update,get,delete}`
/// （AuthRequired + OperationLog 由 router.rs 挂载）。
/// 函数顺序 = 路由顺序 = `list → create → update → get → delete`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["参数配置"])
        .push(Router::with_path("list").post(api::list_configs))
        .push(Router::with_path("create").post(api::create_config))
        .push(Router::with_path("update").post(api::update_config))
        .push(Router::with_path("get").post(api::get_config))
        .push(Router::with_path("delete").post(api::delete_config))
}

/// 网站设置端点：`GET /api/v1/site-config/get`（公开，登录页展示用）+
/// `POST /api/v1/site-config/update`（更新，子路由自挂中间件，router.rs 不再整体挂）。
pub fn site_routes() -> Router {
    Router::new()
        .oapi_tags(["网站设置"])
        .push(Router::with_path("get").get(api::get_site_config))
        .push(
            Router::with_path("update")
                .hoop(AuthRequired)
                .hoop(OperationLog)
                .post(api::update_site_config),
        )
}
