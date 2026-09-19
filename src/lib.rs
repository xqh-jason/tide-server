//! tide-server 库入口：把平台能力（RBAC / 认证 / 字典 / 日志 / 任务 / 文件 / 组织）
//! 作为一个库暴露，供本仓的 bin target（`src/main.rs`）取用。
//!
//! # 为什么有 lib.rs（2026-09-18 新增）
//!
//! 本 crate 原先是 binary-only：`main.rs` 声明 6 个私有 `mod` 后直接起服务。
//! 拆出 lib target 之后，`main.rs` 只声明进程级的事（日志订阅者、配置装载、启动），
//! 业务代码全部留在库里；同时 `cargo test --lib` 可以只跑库侧测试。
//!
//! # 公开面的边界
//!
//! 六个顶层模块整体公开。内部细节（各域的 `validate`、私有辅助函数）仍由
//! 各模块自己的可见性控制，**不因为 lib target 的出现而被动公开**：
//! 各域 `validate` 模块不在 `mod.rs` 里 `pub mod`（见 `modules/AGENTS.md`），
//! 所以仍然拿不到。

pub mod entity;
pub mod infra;
pub mod middleware;
pub mod modules;
pub mod task;
pub mod utils;
