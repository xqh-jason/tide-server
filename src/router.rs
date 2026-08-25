use salvo::prelude::*;

use crate::middleware::InjectState;
use crate::state::AppState;

/// 组装全局路由。后续按模块在 api/v1 下扩展。
pub fn build(state: AppState) -> Router {
    Router::new()
        // 把 AppState 注入每个请求的 Depot，handler 用 `depot.get_typed::<AppState>()` 获取。
        .hoop(InjectState(state))
        .push(Router::with_path("api/v1").push(crate::api::v1::routes()))
}
