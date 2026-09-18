//! 登录日志清理任务：物理删除超过保留期的 `sys_login_log`（handler_name = `cleanup_login_logs`）。
//!
//! 保留期由 `config.log_retention.login_log_days` 控制（缺省 90 天），
//! `0` 表示永久保留（跳过清理）。
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
    let days = state.config.log_retention.login_log_days;
    if days == 0 {
        info!("登录日志保留期配置为 0（永久保留），跳过清理");
        return Ok(());
    }

    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(days as i64);
    let deleted =
        crate::modules::system::login_log::repo::delete_created_before(&state.db, cutoff).await?;
    info!("登录日志清理任务：删除 {deleted} 条超过 {days} 天的登录日志");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cache::MemoryCache;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use std::sync::Arc;

    async fn state_with_days(days: u64) -> AppState {
        let mut config = crate::infra::config::Config::load().unwrap();
        config.log_retention.login_log_days = days;
        let db = Database::connect(&config.database.url).await.unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        AppState::new(config, db, Arc::new(MemoryCache::new()), scheduler)
    }

    /// 每个用例独立的探针用户名（测试并行执行，共用名单会互相干扰）。
    fn probe_name(tag: &str) -> String {
        format!("ulw_ret_{tag}_{}", std::process::id())
    }

    /// 造一条 400 天前的登录日志（超过 90 与 30 两种保留期）。
    ///
    /// 列集合对齐真库 `sys_login_log`：无 `updated_at`（该表不在更新语义内）。
    async fn seed_old_log(state: &AppState, probe: &str) {
        state
            .db
            .execute_unprepared(&format!(
                "INSERT INTO sys_login_log (user_id, username, ip, agent, status, msg, created_at) \
                 VALUES (0, '{probe}', '127.0.0.1', 'probe', 0, 'probe', \
                 DATE_SUB(NOW(), INTERVAL 400 DAY))"
            ))
            .await
            .unwrap();
    }

    async fn probe_count(state: &AppState, probe: &str) -> i64 {
        let row = state
            .db
            .query_one(Statement::from_string(
                DbBackend::MySql,
                format!("SELECT COUNT(*) AS c FROM sys_login_log WHERE username = '{probe}'"),
            ))
            .await
            .unwrap()
            .unwrap();
        use sea_orm::TryGetable;
        i64::try_get(&row, "", "c").unwrap_or(0)
    }

    async fn cleanup(state: &AppState, probe: &str) {
        state
            .db
            .execute_unprepared(&format!(
                "DELETE FROM sys_login_log WHERE username = '{probe}'"
            ))
            .await
            .unwrap();
    }

    /// 保留期语义回归（单用例内顺序断言，避免并行用例互相干扰）。
    ///
    /// 为什么合并为一个用例：`run()` 按 cutoff **全表**删除，无法按探针隔离；
    /// 若拆成两个并行用例，`retention_nonzero` 会把 `retention_zero` 刚插入的
    /// 超旧探针行一起删掉，导致后者前置断言失败（本轮实测踩到）。
    #[tokio::test]
    async fn retention_zero_keeps_rows_while_nonzero_deletes_them() {
        let probe = probe_name("both");
        seed_old_log(&state_with_days(0).await, &probe).await;

        // 阶段 1：保留期 0 → 永久保留，超旧行必须原封不动
        let zero_state = state_with_days(0).await;
        assert_eq!(
            probe_count(&zero_state, &probe).await,
            1,
            "前置：探针行应已写入"
        );
        run(&zero_state).await.unwrap();
        let after_zero = probe_count(&zero_state, &probe).await;
        assert_eq!(after_zero, 1, "0 = 永久保留，超旧日志也不应被删");

        // 阶段 2：同一批数据换成保留期 90 天 → 400 天前的行应被物理删除
        let ninety_state = state_with_days(90).await;
        run(&ninety_state).await.unwrap();
        let after_ninety = probe_count(&ninety_state, &probe).await;
        cleanup(&ninety_state, &probe).await;

        assert_eq!(after_ninety, 0, "超过 90 天的日志应被物理删除");
    }
}
