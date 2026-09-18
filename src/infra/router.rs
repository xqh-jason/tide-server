use std::time::Duration;

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::request_timeout::{self, DEFAULT_REQUEST_TIMEOUT_SECS, RequestTimeout};
use crate::middleware::{
    InjectState, api_permission::ApiPermission, auth::AuthRequired, op_log::OperationLog,
};
use crate::modules::{DomainMount, MountGuard};

/// 按域挂载登记表把业务域出口挂到 `api/v1` 下。
///
/// `domains` 每行含：path 前缀（空串 = 出口自带 path，直接挂 api/v1）、
/// 公开/受保护（Protected 统一挂 AuthRequired + OperationLog + ApiPermission）、
/// 同前缀下并挂的出口集合（如 `/user` 下 user CRUD 与 menu 菜单契约端点）。
///
/// 参数**显式传入**而非直接读 `DOMAINS`：业务仓库注册的域要能一并挂上，
/// 详见 `modules::all_domains` 的说明。
fn mount_domains(api: Router, domains: &[DomainMount]) -> Router {
    let mut api = api;
    for mount in domains {
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

/// 组装全局路由（仅基座内置域）。
///
/// 等价于 `build_with(state, &[])`；基座自己的二进制走这条。
pub fn build(state: AppState) -> Router {
    build_with(state, &[])
}

/// 组装全局路由，并额外挂上 `extra` 里的业务域。
///
/// 业务仓库（以 git 依赖消费本 crate）在自己的 `main.rs` 里把自身的
/// `const MY_DOMAINS: &[DomainMount]` 传进来——见 `modules::all_domains`
/// 的用法说明与 `infra::app::run_with_domains`。
///
/// 中间件与超时豁免对所有域（含 extra）一致生效，业务域无需自行接鉴权。
pub fn build_with(state: AppState, extra: &[DomainMount]) -> Router {
    let domains = crate::modules::all_domains(extra);
    Router::new()
        .hoop(InjectState(state))
        // 请求超时兜底：预算内无害，超时渲染契约体（不产生 4xx/5xx）；文件收发是流式
        // 大对象传输，慢网下天然可能超过预算，按路径豁免
        .hoop_when(
            RequestTimeout::new(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS)),
            |req, _| !request_timeout::is_exempt(req),
        )
        .push(mount_domains(Router::with_path("api/v1"), &domains))
}
