//! 业务域模块（垂直切片）：每域一个目录，内含 api/service/repo/dto。
//! 分两级容器——`system/` 平台能力（随脚手架交付、保持稳定），
//! `biz/` 业务域（具体业务功能从这里生长）；entity/ 保持全局独立
//! （关联表跨域共享，如 sys_user_role）。
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
//! 按钮权限码示例：`system:user:create` 必须同时作为前端按钮权限码和
//! `create_user` 接口的授权依据。
//! 接口级授权层（`ApiPermission` 中间件）：请求按 `path + method` 匹配
//! `sys_api`（未登记放行），已登记接口校验 `sys_role_api` 角色授权、超管短路。
//! 按钮码与接口授权两条通道并存，构成“看得到按钮但调接口被拒”的双保险。
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
//!
//! # 新增业务域装配点（三步）
//!
//! 1. `src/entity/mod.rs` 加 `pub mod <域>;`
//! 2. 容器声明子域：业务域在 `biz/mod.rs`、平台域在 `system/mod.rs` 加 `pub mod <域>;`
//! 3. 本文件末尾 `DOMAINS` 登记表追加一行（path 前缀 + 公开/受保护 + 出口函数），
//!    `infra/router.rs` 据此自动挂载，无需再动路由装配。

pub mod biz;
pub mod system;

use salvo::prelude::Router;

/// 域出口挂载方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountGuard {
    /// 公开出口：不挂登录 / 操作日志 / 接口授权中间件。
    Public,
    /// 受保护出口：挂 `AuthRequired` + `OperationLog` + `ApiPermission` 三件套。
    Protected,
}

/// 一条挂载记录：`path` 为 `api/v1` 下的前缀；空串表示直接挂 `api/v1`（出口自带 path，
/// 如 `system::health::routes()` 自带 `/health`、`system::auth::routes()` 自带 `/auth`）。
/// 同一前缀可并挂多个出口（如 `/user` 下 user CRUD 与 menu 域的菜单契约端点）。
pub struct DomainMount {
    pub path: &'static str,
    pub guard: MountGuard,
    pub routers: &'static [fn() -> Router],
}

/// 域挂载登记表（装配点收敛）：新增域在此追加一行，
/// `infra/router.rs` 据此循环挂载，不再逐域手写 push + 中间件三件套。
/// 顺序贴合历史挂载顺序，便于 diff 对照。
pub const DOMAINS: &[DomainMount] = &[
    // 健康检查：出口自带 `/health`，公开
    DomainMount {
        path: "",
        guard: MountGuard::Public,
        routers: &[system::health::routes],
    },
    // 图形验证码：公开（登录前调用）
    DomainMount {
        path: "captcha",
        guard: MountGuard::Public,
        routers: &[system::captcha::routes],
    },
    // 登录 / 登出：出口自带 `/auth` 前缀，公开
    DomainMount {
        path: "",
        guard: MountGuard::Public,
        routers: &[system::auth::routes],
    },
    // 用户管理 CRUD + 菜单契约端点（POST /api/v1/user/menus，业务在 menu 域）
    DomainMount {
        path: "user",
        guard: MountGuard::Protected,
        routers: &[system::user::routes, system::menu::user_routes],
    },
    // 菜单管理 CRUD：POST /api/v1/menu/{list,create,update,get,delete}
    DomainMount {
        path: "menu",
        guard: MountGuard::Protected,
        routers: &[system::menu::routes],
    },
    // 数据字典类型：POST /api/v1/dictionary/{list,create,update,get,delete,get-by-type}
    DomainMount {
        path: "dictionary",
        guard: MountGuard::Protected,
        routers: &[system::dictionary::routes],
    },
    // 数据字典项：POST /api/v1/dictionary-detail/{list,create,update,get,delete}
    DomainMount {
        path: "dictionary-detail",
        guard: MountGuard::Protected,
        routers: &[system::dictionary::detail_routes],
    },
    // 角色管理：POST /api/v1/role/{list,create,update,get,delete}
    DomainMount {
        path: "role",
        guard: MountGuard::Protected,
        routers: &[system::role::routes],
    },
    // API 管理：POST /api/v1/sys-api/{list,create,update,get,delete}
    DomainMount {
        path: "sys-api",
        guard: MountGuard::Protected,
        routers: &[system::sys_api::routes],
    },
    // 操作日志：POST /api/v1/operation-log/{list,get,delete,delete-batch}
    DomainMount {
        path: "operation-log",
        guard: MountGuard::Protected,
        routers: &[system::operation_log::routes],
    },
    // 登录日志：POST /api/v1/login-log/{list,get,delete,delete-batch}
    DomainMount {
        path: "login-log",
        guard: MountGuard::Protected,
        routers: &[system::login_log::routes],
    },
    // 文件上传：POST /api/v1/file/{list,upload,get,delete} + GET /api/v1/file/download
    DomainMount {
        path: "file",
        guard: MountGuard::Protected,
        routers: &[system::file::routes],
    },
    // 参数配置：POST /api/v1/config/{list,create,update,get,delete}
    DomainMount {
        path: "config",
        guard: MountGuard::Protected,
        routers: &[system::config::routes],
    },
    // 部门管理：POST /api/v1/dept/{list,create,update,get,delete}
    DomainMount {
        path: "dept",
        guard: MountGuard::Protected,
        routers: &[system::dept::routes],
    },
    // 定时任务：POST /api/v1/job/{list,create,update,get,delete,update-status,run-once}
    DomainMount {
        path: "job",
        guard: MountGuard::Protected,
        routers: &[system::job::routes],
    },
    // 执行日志：POST /api/v1/job-log/{list,get,delete,delete-batch}
    DomainMount {
        path: "job-log",
        guard: MountGuard::Protected,
        routers: &[system::job_log::routes],
    },
    // 职位管理：POST /api/v1/position/{list,create,update,get,delete}
    DomainMount {
        path: "position",
        guard: MountGuard::Protected,
        routers: &[system::position::routes],
    },
    // 网站设置：GET /api/v1/site-config/get 公开 + POST update（子路由自挂中间件）
    DomainMount {
        path: "site-config",
        guard: MountGuard::Public,
        routers: &[system::config::site_routes],
    },
    // 刷新凭证：POST /api/v1/refresh-token/{list,delete,delete-batch,force-logout}（在线会话 / 强制下线 / 历史清理）
    DomainMount {
        path: "refresh-token",
        guard: MountGuard::Protected,
        routers: &[system::refresh_token::routes],
    },
];
