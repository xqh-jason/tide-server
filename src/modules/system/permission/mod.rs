//! 权限域：后端授权判定的唯一入口（判定面），本身不拥有数据表。
//!
//! 两条授权通道回答同一个问题「这个用户能不能做这件事」：
//! - 按钮权限码：`sys_menu.permission`（menu_type=3）→ `has_permission`；
//! - 接口资源授权：`sys_api` × `sys_role_api` → `has_api_permission`。
//!
//! `sys_api` 域负责接口资源的登记管理（管理面 CRUD），本域只消费这些数据做判定；
//! 中间件与各 service 的鉴权调用统一收敛到本域的判定函数与常量。

pub mod repo;
pub mod service;

/// 创建系统用户所需的权限码。
///
/// 这里集中定义常量，避免 handler/service/test 中重复手写字符串导致拼写漂移。
pub const SYSTEM_USER_CREATE: &str = "system:user:create";

/// 更新系统用户所需的权限码。
pub const SYSTEM_USER_UPDATE: &str = "system:user:update";

/// 超级管理员角色键（内置保留字）：授权判定短路依据（`has_permission` /
/// `has_api_permission`）；同时作为保留字禁止普通角色创建/改名占用、
/// 禁止非 super 操作者通过用户管理分配给其他用户。
pub const SUPER_ROLE_KEY: &str = "super";

/// 内置超管用户名（`seed` 启动时创建并固定重置密码，不允许被编辑）。
pub const ADMIN_USERNAME: &str = "admin";
