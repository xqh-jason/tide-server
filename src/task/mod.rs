//! 内置定时任务注册表（W6-2）：一个任务一个文件，文件名 = `HANDLER_NAME`。
//!
//! 新增任务的固定动作：建 `src/task/<name>.rs` 实现 `run` + 导出 `HANDLER_NAME`
//! 常量，并在本文件 `handlers()` 注册一行；无需改动 scheduler。任务跨域只调用
//! 各域 repo 函数，不直接操作其他域的 Entity（守「repo 唯一数据访问层」约定）。
//!
//! 规模演进（任务多到几百个时怎么走，注册表本身不是瓶颈——静态 HashMap 查找
//! O(1)，调度器 tick 遍历几百个任务是微秒级）：
//! - 文件多了按域分目录（如 `task/cleanup/`），`HANDLER_NAME` 仍是扁平键，
//!   不受目录影响；
//! - 注册表保持显式手动：本文件即「全局任务索引」，一眼可见系统有哪些任务；
//!   键引用各文件 `HANDLER_NAME` 常量，拼错即编译失败；
//! - 不引入 linkme 宏自动注册（注册关系不可见 + 多一个依赖）与 DB 驱动扩展
//!   （handler 是代码，必须编译期注册；DB 只存「哪个任务用哪个 handler +
//!   什么时候跑」，即 `sys_job.handler_name` + `cron_expr`，分工已就位）。

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
            // 登录日志清理任务
            // 登录日志清理任务：物理删除 90 天前的 `sys_login_log`（handler_name = `cleanup_login_logs`）。
            (
                login_log_cleanup::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(login_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
            // 调度日志清理任务
            // 调度日志清理任务：物理删除 180 天前的 `sys_job_log`（handler_name = `cleanup_job_logs`，
            (
                job_log_cleanup::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(job_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 守卫：内置 handler 必须已注册——防止重构手滑删掉注册行，
    /// 导致该 handler 名建任务时报「任务处理器不存在」的静默不可用。
    #[test]
    fn handlers_registry_contains_builtin_handlers() {
        assert!(handlers().contains_key(login_log_cleanup::HANDLER_NAME));
        assert!(handlers().contains_key(job_log_cleanup::HANDLER_NAME));
    }
}
