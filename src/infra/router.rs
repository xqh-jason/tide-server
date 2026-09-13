use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::{
    InjectState, api_permission::ApiPermission, auth::AuthRequired, op_log::OperationLog,
};
use crate::modules::{DOMAINS, MountGuard};

/// 按挂载登记表把业务域出口挂到 `api/v1` 下。
///
/// `DOMAINS` 每行含：path 前缀（空串 = 出口自带 path，直接挂 api/v1）、
/// 公开/受保护（Protected 统一挂 AuthRequired + OperationLog + ApiPermission）、
/// 同前缀下并挂的出口集合（如 `/user` 下 user CRUD 与 menu 菜单契约端点）。
fn mount_domains(api: Router) -> Router {
    let mut api = api;
    for mount in DOMAINS {
        let mut inner = Router::new();
        if mount.guard == MountGuard::Protected {
            inner = inner
                .hoop(AuthRequired)
                .hoop(OperationLog)
                .hoop(ApiPermission);
        }
        for routes in mount.routers {
            inner = inner.push(routes());
        }
        api = if mount.path.is_empty() {
            api.push(inner)
        } else {
            api.push(Router::with_path(mount.path).push(inner))
        };
    }
    api
}

/// 组装全局路由。业务域挂载点已收敛到 `modules::DOMAINS` 登记表，
/// 新增域只改登记表，无需再动此处。
pub fn build(state: AppState) -> Router {
    Router::new()
        .hoop(InjectState(state))
        .push(mount_domains(Router::with_path("api/v1")))
}
