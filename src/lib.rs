//! tide-server 库入口：把平台能力（RBAC / 认证 / 字典 / 日志 / 任务 / 文件 / 组织）
//! 作为**可复用的基座**对外暴露，供外部项目以自己的 binary 消费。
//!
//! # 为什么有 lib.rs（2026-09-18 新增）
//!
//! 本 crate 原先是 binary-only：`main.rs` 声明 6 个私有 `mod` 后直接起服务。
//! 那个形态对「单可部署二进制」是自洽的，但**无法被其它 crate 依赖** ——
//! 想要复用平台能力的项目拿不到任何公开符号，只能 fork 整个仓库，从此两边分叉。
//!
//! 拆出 lib target 之后有两种消费方式：
//!
//! 1. **直接用基座起服务**：外部项目的 `main.rs` 调 [`infra::app::run_with_domains`]，
//!    自己的业务域通过 `modules::DomainMount` 注册进来（见 `modules` 模块文档）；
//! 2. **只用基座的部分能力**：如 `tide_server::utils::error::AppError`、
//!    `tide_server::entity::sys_user`，在自己的服务里复用约定与实体。
//!
//! 注意本 crate 同时保留 bin target（`main.rs`），因此 `cargo run` 仍然
//! 直接起一个完整的开箱即用服务，行为与拆分前一致。
//!
//! # 公开面的边界
//!
//! 六个顶层模块整体公开。内部细节（各域的 `validate`、私有辅助函数）仍由
//! 各模块自己的可见性控制，**不因为 lib target 的出现而被动公开**：
//! 各域 `validate` 模块不在 `mod.rs` 里 `pub mod`（见 `modules/AGENTS.md`），
//! 所以外部依旧拿不到。
//!
//! # 稳定性承诺
//!
//! 外部项目以 **git 依赖 + 版本 tag** 消费本 crate。当前阶段只保证
//! 「同一 tag 内不变」，不承诺跨 tag 的 API 兼容性 —— 基座仍在演进，
//! 过早冻结公开面会阻碍它变好。消费方升级 tag 时应预期到需要小改。

pub mod entity;
pub mod infra;
pub mod middleware;
pub mod modules;
pub mod task;
pub mod utils;
