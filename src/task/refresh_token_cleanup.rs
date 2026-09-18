//! 刷新凭证清理任务：物理删除过期超过保留期的 `sys_refresh_token`（handler_name = `cleanup_refresh_tokens`）。
//!
//! 保留期由 `config.log_retention.refresh_token_days` 控制（缺省 30 天），
//! `0` 表示永久保留（跳过清理）。保留死会话供审计（谁在何时被强制下线），
//! 过期即吊销语义不受影响——认证中间件按 `expires_at > NOW` 判定可用性，
//! 过期会话本就无法通过。
//! 数据访问走 session 域 repo，跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "cleanup_refresh_tokens";

/// handler 中文显示名（前端下拉 label，与 [`HANDLER_NAME`] 成对提供）。
pub const HANDLER_LABEL: &str = "清理刷新凭证";

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let days = state.config.log_retention.refresh_token_days;
    if days == 0 {
        info!("刷新凭证保留期配置为 0（永久保留），跳过清理");
        return Ok(());
    }

    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(days as i64);
    let deleted =
        crate::modules::system::refresh_token::repo::delete_expired_before(&state.db, cutoff)
            .await?;
    info!("刷新凭证清理任务：删除 {deleted} 条过期超过 {days} 天的用户会话");
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
        config.log_retention.refresh_token_days = days;
        let db = Database::connect(&config.database.url).await.unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        AppState::new(config, db, Arc::new(MemoryCache::new()), scheduler)
    }

    /// 每条用例独立的探针 hash（测试并行，共用会互相干扰）。
    fn probe_hash(tag: &str) -> String {
        format!("ulw_rt_{tag}_{}", std::process::id())
    }

    /// 造一条「早已过期」的会话（过期 100 天，超过 30 天保留期）。
    async fn seed_expired(state: &AppState, hash: &str) {
        state
            .db
            .execute_unprepared(&format!(
                "INSERT INTO sys_refresh_token \
                 (user_id, username, refresh_token_hash, ip, agent, last_active_at, expires_at, created_at, updated_at, revoked_by, revoke_reason) \
                 VALUES (0, 'ulw_rt_probe', '{hash}', '127.0.0.1', 'probe', \
                 DATE_SUB(NOW(), INTERVAL 200 DAY), DATE_SUB(NOW(), INTERVAL 100 DAY), \
                 DATE_SUB(NOW(), INTERVAL 200 DAY), DATE_SUB(NOW(), INTERVAL 200 DAY), 0, '')"
            ))
            .await
            .unwrap();
    }

    async fn probe_count(state: &AppState, hash: &str) -> i64 {
        let row = state
            .db
            .query_one(Statement::from_string(
                DbBackend::MySql,
                format!(
                    "SELECT COUNT(*) AS c FROM sys_refresh_token WHERE refresh_token_hash = '{hash}'"
                ),
            ))
            .await
            .unwrap()
            .unwrap();
        use sea_orm::TryGetable;
        i64::try_get(&row, "", "c").unwrap_or(0)
    }

    async fn cleanup(state: &AppState, hash: &str) {
        state
            .db
            .execute_unprepared(&format!(
                "DELETE FROM sys_refresh_token WHERE refresh_token_hash = '{hash}'"
            ))
            .await
            .unwrap();
    }

    /// 保留期语义回归（单用例内顺序断言，避免并行用例互相干扰）。
    ///
    /// `run()` 按 cutoff **全表**删除，无法按探针隔离；拆成并行用例时
    /// `nonzero` 会删掉 `zero` 刚插入的超旧行（本轮实测踩到）。
    #[tokio::test]
    async fn retention_zero_keeps_rows_while_nonzero_deletes_them() {
        let probe = probe_hash("both");
        seed_expired(&state_with_days(0).await, &probe).await;

        // 阶段 1：保留期 0 → 永久保留，早已过期的会话也不得删
        let zero_state = state_with_days(0).await;
        assert_eq!(
            probe_count(&zero_state, &probe).await,
            1,
            "前置：探针行应已写入"
        );
        run(&zero_state).await.unwrap();
        assert_eq!(
            probe_count(&zero_state, &probe).await,
            1,
            "0 = 永久保留，过期会话也不应被删"
        );

        // 阶段 2：同一批数据换成保留期 30 天 → 过期 100 天的会话应被删
        let thirty_state = state_with_days(30).await;
        run(&thirty_state).await.unwrap();
        let after = probe_count(&thirty_state, &probe).await;
        cleanup(&thirty_state, &probe).await;

        assert_eq!(after, 0, "过期超过 30 天的会话应被物理删除");
    }
}
