//! 权限域：把数据库中的有效按钮权限码转换成接口鉴权依据。

pub mod repo;
pub mod service;

/// 创建系统用户所需的权限码。
///
/// 这里集中定义常量，避免 handler/service/test 中重复手写字符串导致拼写漂移。
pub const SYSTEM_USER_CREATE: &str = "system:user:create";

/// 超级管理员角色键，项目约定拥有全部权限（授权短路依据）。
pub const SUPER_ROLE_KEY: &str = "super";
