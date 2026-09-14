//! 登录日志清理任务：物理删除 90 天前的 `sys_login_log`（handler_name = `cleanup_login_logs`）。
//!
//! 数据访问走 login_log 域 repo，跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "cleanup_login_logs";

/// handler 中文显示名（前端下拉 label，与 [`HANDLER_NAME`] 成对提供）。
pub const HANDLER_LABEL: &str = "清理登录日志";

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(90);
    let deleted =
        crate::modules::system::login_log::repo::delete_created_before(&state.db, cutoff).await?;
    info!("登录日志清理任务：删除 {} 条过期登录日志", deleted);
    Ok(())
}
