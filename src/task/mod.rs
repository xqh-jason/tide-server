//! 内置定时任务注册表（W6-2）：一个任务一个文件，文件名 = `handler_name`。
//!
//! 新增任务的固定动作：建 `src/task/<name>.rs` 实现 `run`，并在本文件 `handlers()`
//! 注册一行；无需改动 scheduler。任务跨域只调用各域 repo 函数，
//! 不直接操作其他域的 Entity（守「repo 唯一数据访问层」约定）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use crate::infra::state::AppState;

pub mod job_log_cleanup;
pub mod login_log_cleanup;

/// 内置任务处理器签名：async fn(&AppState) -> anyhow::Result<()> 装箱为 BoxFuture。
pub type JobHandler =
    fn(&AppState) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>;

/// 内置 handler 注册表：键 = `sys_job.handler_name`，CRUD 时校验任务必须命中。
pub fn handlers() -> &'static HashMap<&'static str, JobHandler> {
    static REG: OnceLock<HashMap<&'static str, JobHandler>> = OnceLock::new();
    REG.get_or_init(|| {
        HashMap::from([
            (
                "cleanup_login_logs",
                (|state: &AppState| {
                    Box::pin(login_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
            (
                "cleanup_job_logs",
                (|state: &AppState| {
                    Box::pin(job_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
        ])
    })
}
