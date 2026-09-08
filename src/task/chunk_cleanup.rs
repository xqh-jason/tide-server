//! 分片清理任务：删除超过 `[upload].chunk_retain_hours`（默认 24h）未合并的
//! 断点续传分片（目录 + `sys_file_chunk` 记录，handler_name = `cleanup_file_chunks`）。
//! 数据访问走 file 域 repo（`find_expired_chunk_md5s`），目录与记录删除复用
//! file 域 service 的幂等 `remove_chunks`（与手动放弃上传同一管道）。

use tracing::info;

use crate::infra::state::AppState;

/// handler 名（= 文件名 = `sys_job.handler_name` 的合法值，三处保持一致）。
pub const HANDLER_NAME: &str = "cleanup_file_chunks";

/// 任务入口。
pub async fn run(state: &AppState) -> anyhow::Result<()> {
    // 实现步骤（W6-3 任务 7）：
    // 1. cutoff = now - state.config.upload.chunk_retain_hours 小时；
    // 2. file_repo::find_expired_chunk_md5s(&state.db, cutoff) 取过期会话；
    // 3. 逐 md5 调 file_service::remove_chunks（幂等删目录 + 删记录）：
    //    成功 info!("分片清理任务：会话 {md5} 删除 {n} 条分片")；
    //    单会话失败仅 warn 继续（不中断整轮，下轮重试）。
    let _ = state;
    todo!("W6-3 任务 7：分片清理任务入口")
}
