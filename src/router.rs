use salvo::prelude::*;

use crate::state::AppState;

/// 组装全局路由。后续按模块在 api/v1 下扩展。
pub fn build(_state: AppState) -> Router {
    Router::new().push(Router::with_path("api/v1").push(crate::api::v1::routes()))
}
