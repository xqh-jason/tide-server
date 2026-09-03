use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::{InjectState, auth::AuthRequired, op_log::OperationLog};

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
                        .hoop(OperationLog)
                        .push(crate::modules::user::routes())
                        // 菜单契约端点：POST /api/v1/user/menus（业务在 menu 域）
                        .push(crate::modules::menu::user_routes()),
                )
                // 菜单管理 CRUD：POST /api/v1/menu/{list,create,update,get,delete}
                .push(
                    Router::with_path("menu")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::menu::routes()),
                )
                // 数据字典管理：POST /api/v1/dict/{list,create,update,get,delete}（codegen 生成域）
                .push(
                    Router::with_path("dict")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::dict::routes()),
                )
                .push(
                    Router::with_path("role")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::role::routes()),
                )
                .push(
                    Router::with_path("sys-api")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::sys_api::routes()),
                )
                // 操作日志：POST /api/v1/operation-log/{list,get,delete,delete-batch}
                .push(
                    Router::with_path("operation-log")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::operation_log::routes()),
                )
                // 登录日志：POST /api/v1/login-log/{list,get,delete,delete-batch}
                .push(
                    Router::with_path("login-log")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::login_log::routes()),
                ),
        )
}
