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
//!
//! # 接口命名约定（按层统一）
//!
//! | 层 | 分页 | CRUD |
//! |---|---|---|
//! | repo（数据访问） | `find_page` | `find_by_id` / `create_*` / `update_*` / `soft_delete_*` / `find_by_*_include_deleted` |
//! | service（业务） | `page_<实体>` | `create_<实体>` / `update_<实体>` / `get_<实体>` / `delete_<实体>` |
//! | handler（端点） | `list_<实体>` | `create_<实体>` / `update_<实体>` / `get_<实体>` / `delete_<实体>` |
//!
//! 同一域内 `api.rs` 函数顺序 = `mod.rs` 路由挂载顺序 = `list → create → update → get → delete`；
//! 特殊契约端点（`info`、`access-codes`、`menus` 等）排在 CRUD 之后。
//!
//! repo 分页查询的过滤参数统一打包为域 `*Filter` 结构体（如 `UserFilter` / `RoleFilter` /
//! `MenuFilter`），与分页参数（`page_index` / `page_size`）分离；加过滤条件只改 Filter，
//! repo 签名与调用点不变。Filter 定义在对应域 `dto.rs`。
//!
//! 涉及关联表写入的 repo 函数统一使用 `<动词>_<实体>_with_links` 后缀
//! （如 `create_role_with_links` / `create_api_with_links`），无关联直接 `<动词>_<实体>`。

pub mod auth;
pub mod captcha;
pub mod dictionary;
pub mod file;
pub mod login_log;
pub mod menu;
pub mod operation_log;
pub mod permission;
pub mod role;
pub mod sys_api;
pub mod system;
pub mod user;
