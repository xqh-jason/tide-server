//! 权限域：把数据库中的有效按钮权限码转换成接口鉴权依据。

pub mod repo;
pub mod service;

/// 创建系统用户所需的权限码。
///
/// 这里集中定义常量，避免 handler/service/test 中重复手写字符串导致拼写漂移。
pub const SYSTEM_USER_CREATE: &str = "system:user:create";

/// 更新系统用户所需的权限码。
pub const SYSTEM_USER_UPDATE: &str = "system:user:update";

/// 超级管理员角色键，项目约定拥有全部权限（授权短路依据）。
pub const SUPER_ROLE_KEY: &str = "super";

/// 内置超管用户名（`seed` 启动时创建并固定重置密码，不允许被编辑）。
pub const ADMIN_USERNAME: &str = "admin";

/// 内置超管角色键（`seed` 启动时创建并固定重置密码，不允许被编辑）。
pub const SUPER_ADMIN_ROLE_KEY: &str = "super";
