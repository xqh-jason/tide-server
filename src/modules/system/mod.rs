//! 系统域：健康检查等系统级接口。W6 起并入服务器监控、定时任务管理。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

pub mod api;

/// 系统域路由：`POST /api/v1/health`（W6 起在此并入服务器监控、定时任务管理）。
pub fn routes() -> Router {
    Router::with_path("health")
        .oapi_tags(["系统"])
        .post(api::health)
}
