//! 登录日志清理任务：物理删除 90 天前的 `sys_login_log`（handler_name = `cleanup_login_logs`）。
//!
//! 数据访问走 login_log 域 repo，跨域不直接操作 Entity。

use tracing::info;

use crate::infra::state::AppState;

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(90);
    let deleted = crate::modules::login_log::repo::delete_created_before(&state.db, cutoff).await?;
    info!("删除 {} 条过期登录日志", deleted);
    Ok(())
}
