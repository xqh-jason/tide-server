use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::{auth::AuthRequired, InjectState};

/// 组装全局路由。系统域在 modules/system，业务域在 modules/*。
pub fn build(state: AppState) -> Router {
    Router::new()
        // 把 AppState 注入每个请求的 Depot，handler 用 `depot.get_typed::<AppState>()` 获取。
        .hoop(InjectState(state))
        .push(
            Router::with_path("api/v1")
                .push(crate::modules::system::routes())
                .push(crate::modules::auth::routes())
                // W2 起 user 域整体需要登录（login/health 公开，不挂本中间件）
                .push(
                    Router::with_path("user")
                        .hoop(AuthRequired)
                        .push(crate::modules::user::routes()),
                ),
        )
}
