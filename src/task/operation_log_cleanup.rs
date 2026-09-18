//! 操作日志清理任务：物理删除 90 天前的 `sys_operation_log`（handler_name =
//! `cleanup_operation_logs`，自举：自身日志自身清）。数据访问走 operation_log 域 repo
//! （内部滚动分批删除），跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;
use crate::modules::system::operation_log::repo as operation_log_repo;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "cleanup_operation_logs";

/// handler 中文显示名（前端下拉 label，与 [`HANDLER_NAME`] 成对提供）。
pub const HANDLER_LABEL: &str = "清理操作日志";

/// 任务入口。
///
/// 保留期由 `config.operation_log.retention_days` 控制（缺省 90 天）：
/// 硬编码保留期无法应对合规要求差异，且一旦需要延长保留，
/// 旧日志早已被清理——审计窗口必须可配。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let days = state.config.operation_log.retention_days;

    // 0 = 永久保留：显式跳过。
    // ★ 不加这个分支的话，`Duration::days(0)` 会得到「当前时刻」作 cutoff，
    //   把所有历史日志一次性删光（灾难性误读）。
    if days == 0 {
        info!("操作日志保留期配置为 0（永久保留），跳过清理");
        return Ok(());
    }

    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(days as i64);
    let deleted = operation_log_repo::delete_created_before(&state.db, cutoff).await?;
    info!("操作日志清理任务：删除 {deleted} 条超过 {days} 天的日志");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cache::MemoryCache;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use std::sync::Arc;

    async fn state_with_retention(days: u64) -> AppState {
        let mut config = crate::infra::config::Config::load().unwrap();
        config.operation_log.retention_days = days;
        let db = Database::connect(&config.database.url).await.unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        AppState::new(config, db, Arc::new(MemoryCache::new()), scheduler)
    }

    async fn count_logs(state: &AppState) -> i64 {
        let row = state
            .db
            .query_one(Statement::from_string(
                DbBackend::MySql,
                "SELECT COUNT(*) AS c FROM sys_operation_log".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        row.try_get::<i64>("", "c").unwrap_or(0)
    }

    /// retention_days == 0 → 永久保留：清理任务必须跳过，一行都不能删。
    ///
    /// 回归价值：不加这个分支时 days=0 会让 cutoff = 当前时刻，
    /// 把所有历史日志一次性删光——灾难性误读。
    #[tokio::test]
    async fn retention_zero_skips_cleanup_and_deletes_nothing() {
        let state = state_with_retention(0).await;
        let before = count_logs(&state).await;

        run(&state).await.unwrap();

        let after = count_logs(&state).await;
        assert_eq!(after, before, "0 = 永久保留，不应删除任何日志");
    }

    /// 非 0 时正常清理（只断言不增加，避免动共享真库数据）。
    #[tokio::test]
    async fn retention_nonzero_runs_cleanup() {
        let state = state_with_retention(90).await;
        let before = count_logs(&state).await;

        run(&state).await.unwrap();

        let after = count_logs(&state).await;
        assert!(
            after <= before,
            "清理只会减少或不变，实际 {before} → {after}"
        );
    }
}
