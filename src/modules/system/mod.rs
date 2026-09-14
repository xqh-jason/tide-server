//! 平台能力容器：随脚手架交付的系统能力域（RBAC / 认证 / 字典 / 日志 / 任务等），
//! 与业务域（`crate::modules::biz`）分离——业务开发只往 biz/ 里加域，本组保持稳定。
//!
//! 组内仍按垂直切片组织：每域 `{api, service, repo, dto}` 四件套，
//! 按需裁剪（如日志域只读 + 删除、auth 无 repo）。挂载统一走顶层
//! `modules/mod.rs` 的 `DOMAINS` 登记表，本文件只做子域声明。

pub mod auth;
pub mod captcha;
pub mod config;
pub mod dept;
pub mod dictionary;
pub mod file;
pub mod health;
pub mod job;
pub mod job_log;
pub mod login_log;
pub mod menu;
pub mod operation_log;
pub mod permission;
pub mod position;
pub mod refresh_token;
pub mod role;
pub mod sys_api;
pub mod user;
