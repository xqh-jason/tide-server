//! 全局中间件。W2 起在此添加 JWT 认证、权限校验、CORS、日志、Recovery。

use salvo::prelude::*;

use crate::infra::state::AppState;

pub mod auth;

/// 状态注入中间件：把 `AppState` 按类型（`insert_typed`）存入每个请求的 Depot，
/// handler 中通过 `depot.get_typed::<AppState>()` 获取。
///
/// 等价于官方 `affix_state::inject`（后者依赖 salvo_extra 0.95.2，rsproxy 镜像暂无，
/// 手写 3 行更可控，也便于理解 Depot 机制）。
pub struct InjectState(pub AppState);

#[async_trait]
impl Handler for InjectState {
    async fn handle(
        &self,
        _req: &mut Request,
        depot: &mut Depot,
        _res: &mut Response,
        _ctrl: &mut FlowCtrl,
    ) {
        depot.insert_typed(self.0.clone());
    }
}
