//! 刷新凭证清理任务：物理删除过期超 30 天的 `sys_refresh_token`（handler_name = `cleanup_refresh_tokens`）。
//!
//! 保留 30 天死会话供审计（谁在何时被强制下线），过期即吊销语义不受影响——
//! 认证中间件按 `expires_at > NOW` 判定可用性，过期会话本就无法通过。
//! 数据访问走 session 域 repo，跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "cleanup_refresh_tokens";

/// handler 中文显示名（前端下拉 label，与 [`HANDLER_NAME`] 成对提供）。
pub const HANDLER_LABEL: &str = "清理刷新凭证";

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(30);
    let deleted =
        crate::modules::system::refresh_token::repo::delete_expired_before(&state.db, cutoff)
            .await?;
    info!(
        "刷新凭证清理任务：删除 {} 条过期超 30 天的用户会话",
        deleted
    );
    Ok(())
}
