//! 调度日志清理任务：物理删除 90 天前的 `sys_job_log`（handler_name =
//! `cleanup_job_logs`，自举：自身日志自身清）。数据访问走 job_log 域 repo
//! （内部滚动分批删除），跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;
use crate::modules::system::job_log::repo as job_log_repo;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "cleanup_job_logs";

/// handler 中文显示名（前端下拉 label，与 [`HANDLER_NAME`] 成对提供）。
pub const HANDLER_LABEL: &str = "清理调度日志";

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(90);
    let deleted = job_log_repo::delete_created_before(&state.db, cutoff).await?;
    info!("调度日志清理任务：删除 {deleted} 条过期调度日志");
    Ok(())
}
