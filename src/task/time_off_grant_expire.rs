//! 假期额度批次过期作废任务（handler_name = `time_off_grant_expire`）。
//!
//! 每日扫描「`expire_at < 今天` 且仍有剩余」的额度批次：`remaining_minutes` 归零 +
//! `status` 置为已失效，作废量计入账户 `expired_minutes`，并逐批次写 `biz_type = 6` 的
//! 过期流水。幂等：扫描条件带 `remaining_minutes > 0`，重复执行不会二次记账。
//! 数据访问走 time_off 域 service（跨域不直接操作 Entity）。
//!
//! `sys_job` 种子行（`cron_expr = "0 30 1 * * *"`，每日 01:30）由任务 4 追加。

use crate::infra::state::AppState;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "time_off_grant_expire";

/// handler 中文显示名（前端下拉 label，与 [`HANDLER_NAME`] 成对提供）。
pub const HANDLER_LABEL: &str = "假期额度过期作废";

/// 任务入口。
// 实现提示：① `let today = chrono::Local::now().date_naive();`；② `let txn = state.db.begin().await?;`
// → `crate::modules::biz::hr::time_off::service::expire_grants_in_tx(&txn, today).await`
// （`AppError` → `anyhow::Error`：`map_err(anyhow::Error::from)?`）→ 成功 `txn.commit().await?`；
// ③ `tracing::info!("假期额度过期作废任务：作废 {count} 个批次")`——任务执行结果行由 scheduler
// 统一写 `sys_job_log`，本函数不落库日志。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    let _ = state;
    Err(anyhow::anyhow!("未实现：time_off_grant_expire::run"))
}
