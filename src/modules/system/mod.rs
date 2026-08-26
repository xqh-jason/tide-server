//! 系统域：健康检查等系统级接口。W6 起并入服务器监控、定时任务管理。

use salvo::prelude::*;

pub mod api;

pub fn routes() -> Router {
    Router::with_path("health").get(api::health)
}
