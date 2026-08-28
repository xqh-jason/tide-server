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
//!
//! # 权限码约定
//!
//! `sys_menu.permission` 是前端按钮与后端授权点的共享语义标识。前端通过
//! `/access-codes` 返回的权限码控制按钮显隐；后端必须在受保护接口中独立校验
//! 相同权限码。前端控制只影响交互体验，不能替代后端鉴权。
//!
//! 第一阶段采用“按钮权限码 + 后端显式校验”的模式：例如 `system:user:create`
//! 必须同时作为前端按钮权限码和 `create_user` 接口的授权依据。
//! `sys_api` 与 `sys_role_api` 仅保留表结构，不作为 W3 第一版的主授权数据源；
//! 后续如需接口级集中授权，再引入接口与权限码的正式映射。

pub mod auth;
pub mod menu;
pub mod permission;
pub mod role;
pub mod sys_api;
pub mod system;
pub mod user;
