//! 业务域模块（垂直切片）：每个业务域一个目录，内含 api/service/repo/dto。
//! entity/ 保持全局独立（关联表跨域共享，如 sys_user_role）。
//!
//! # 数据有效性约定
//!
//! 主表使用软删除：`sys_user`、`sys_role`、`sys_menu`、`sys_api` 的
//! `deleted_at IS NULL` 表示有效数据。除非函数名或调用场景明确要求读取历史 /
//! 回收站数据（如 `include_deleted`），否则业务查询默认必须排除软删除记录。
//!
//! 关系表 `sys_user_role`、`sys_role_menu`、`sys_role_api` 采用硬删除；它们自身
//! 不判断 `deleted_at`，但通过关系表读取主表数据时，仍必须过滤主表软删除条件。

pub mod auth;
pub mod menu;
pub mod role;
pub mod sys_api;
pub mod system;
pub mod user;
