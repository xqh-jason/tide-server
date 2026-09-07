//! 调度日志清理任务：物理删除 180 天前的 `sys_job_log`（handler_name = `cleanup_job_logs`，
//! 自举：自身日志自身清）。数据访问走 job_log 域 repo，跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(180);
    let deleted = crate::modules::job_log::repo::delete_created_before(&state.db, cutoff).await?;
    info!("删除 {} 条过期调度日志", deleted);
    Ok(())
}
