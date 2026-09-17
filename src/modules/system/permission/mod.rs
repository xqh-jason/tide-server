//! 权限域：后端授权判定的唯一入口（判定面），本身不拥有数据表。
//!
//! 后端授权只有一条通道：接口资源授权（`sys_api` × `sys_role_api`）→ `has_api_permission`，
//! 由 `middleware/api_permission.rs` 在每个受保护请求上消费。
//!
//! `sys_menu.permission` 的按钮权限码不参与后端判定：它经 `/access-codes` 下发给前端，
//! 只控制按钮显隐（判定面不消费）。原 service 层按钮码校验（`has_permission` + 域常量）
//! 已于 2026-09-17 连根删除，勿再按「双通道/双保险」重建。
//!
//! `sys_api` 域负责接口资源的登记管理（管理面 CRUD），本域只消费这些数据做判定；
//! 中间件与各 service 的鉴权调用统一收敛到本域的判定函数与常量。

pub mod repo;
pub mod service;

/// 超级管理员角色键（内置保留字）：授权判定短路依据（`has_api_permission`）；
/// 同时作为保留字禁止普通角色创建/改名占用、
/// 禁止非 super 操作者通过用户管理分配给其他用户。
pub const SUPER_ROLE_KEY: &str = "super";

/// 内置超管用户名（`seed` 启动时创建并固定重置密码，不允许被编辑）。
pub const ADMIN_USERNAME: &str = "admin";
