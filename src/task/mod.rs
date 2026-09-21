//! 内置定时任务注册表：一个任务一个文件，文件名 = `HANDLER_NAME`。
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
pub mod leave_grant_expire;
pub mod login_log_cleanup;
pub mod operation_log_cleanup;
pub mod refresh_token_cleanup;

/// 内置任务处理器签名：async fn(&AppState) -> anyhow::Result<()> 装箱为 BoxFuture。
pub type JobHandler =
    fn(&AppState) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>;

/// 处理器展示信息：`name` = 注册表键（= `sys_job.handler_name` 合法值），
/// `label` = 中文显示名，供前端下拉直接渲染，无需前端写死映射。
pub struct HandlerDef {
    pub name: &'static str,
    pub label: &'static str,
}

/// 内置 handler 下拉数据源：顺序即前端下拉顺序，与 [`handlers`] 注册对齐。
///
/// 与 [`handlers`] 分列两处，但都引用各任务文件的 `HANDLER_NAME` / `HANDLER_LABEL`
/// 常量（拼错即编译失败）；[`tests::handler_defs_matches_registry`] 守卫两者不漂移。
pub fn handler_defs() -> &'static [HandlerDef] {
    static DEFS: OnceLock<Vec<HandlerDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        vec![
            HandlerDef {
                name: login_log_cleanup::HANDLER_NAME,
                label: login_log_cleanup::HANDLER_LABEL,
            },
            HandlerDef {
                name: job_log_cleanup::HANDLER_NAME,
                label: job_log_cleanup::HANDLER_LABEL,
            },
            HandlerDef {
                name: operation_log_cleanup::HANDLER_NAME,
                label: operation_log_cleanup::HANDLER_LABEL,
            },
            HandlerDef {
                name: refresh_token_cleanup::HANDLER_NAME,
                label: refresh_token_cleanup::HANDLER_LABEL,
            },
            HandlerDef {
                name: leave_grant_expire::HANDLER_NAME,
                label: leave_grant_expire::HANDLER_LABEL,
            },
        ]
    })
}

/// 内置 handler 注册表：键 = `sys_job.handler_name`，CRUD 时校验任务必须命中。
pub fn handlers() -> &'static HashMap<&'static str, JobHandler> {
    static REG: OnceLock<HashMap<&'static str, JobHandler>> = OnceLock::new();
    REG.get_or_init(|| {
        HashMap::from([
            // 登录日志清理任务（见 login_log_cleanup.rs）
            (
                login_log_cleanup::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(login_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
            // 调度日志清理任务（见 job_log_cleanup.rs）
            (
                job_log_cleanup::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(job_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
            // 操作日志清理任务（见 operation_log_cleanup.rs）
            (
                operation_log_cleanup::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(operation_log_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
            // 会话清理任务（见 refresh_token_cleanup.rs）
            (
                refresh_token_cleanup::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(refresh_token_cleanup::run(state))
                        as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>
                }) as JobHandler,
            ),
            // 假期额度过期作废任务（见 leave_grant_expire.rs）
            (
                leave_grant_expire::HANDLER_NAME,
                (|state: &AppState| {
                    Box::pin(leave_grant_expire::run(state))
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
        assert!(handlers().contains_key(operation_log_cleanup::HANDLER_NAME));
        assert!(handlers().contains_key(refresh_token_cleanup::HANDLER_NAME));
        assert!(handlers().contains_key(leave_grant_expire::HANDLER_NAME));
    }

    /// 守卫：`handler_defs` 与注册表 key 一一对应、label 非空——防止新增/删除
    /// handler 时只改一处，导致前端下拉漏项或多出「注册表里不存在」的选项。
    #[test]
    fn handler_defs_matches_registry() {
        let registry = handlers();
        let defs = handler_defs();

        assert_eq!(
            defs.len(),
            registry.len(),
            "defs 与注册表数量应一致（新增 handler 需同步 label）"
        );
        for def in defs {
            assert!(
                registry.contains_key(def.name),
                "defs 中的 name 必须在注册表内：{}",
                def.name
            );
            assert!(!def.label.is_empty(), "label 不应为空：{}", def.name);
        }
    }
}
