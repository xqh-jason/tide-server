//! 业务域模块（垂直切片）：每个业务域一个目录，内含 api/service/repo/dto。
//! entity/ 保持全局独立（关联表跨域共享，如 sys_user_role）。

pub mod system;
pub mod user;
