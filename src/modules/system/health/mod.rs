//! 系统域：健康检查等系统级接口（服务器监控已取消，定时任务在 job 域）。

use salvo::prelude::*;

pub mod api;

/// 系统域路由：`POST /api/v1/health`。
pub fn routes() -> Router {
    Router::with_path("health")
        .oapi_tags(["系统"])
        .post(api::health)
}
