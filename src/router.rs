use salvo::prelude::*;

use crate::middleware::InjectState;
use crate::state::AppState;

/// 组装全局路由。系统级接口在 api/v1，业务接口在各域模块（modules/*）。
pub fn build(state: AppState) -> Router {
    Router::new()
        // 把 AppState 注入每个请求的 Depot，handler 用 `depot.get_typed::<AppState>()` 获取。
        .hoop(InjectState(state))
        .push(
            Router::with_path("api/v1")
                .push(crate::api::v1::routes())
                .push(crate::modules::user::routes()),
        )
}
