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
//! 后端授权只有一条通道：接口级授权（`ApiPermission` 中间件）——请求按
//! `path + method` 匹配 `sys_api`（未登记放行），已登记接口校验 `sys_role_api`
//! 角色授权、超管短路；判定内核在 `permission::service::has_api_permission`。
//!
//! `sys_menu.permission` 的按钮权限码只控前端按钮显隐：经 `/access-codes` 下发，
//! 不参与后端判定（service 层按钮码校验已于 2026-09-17 删除，勿重建。）
//! 原因是它与接口授权是两张各自维护的授权表，同时生效等于同一动作必须两处授权
//! 同时命中，只会制造「授了接口却仍被拒」的静默失败，并不提供额外边界。
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
///
/// `Clone` / `Debug` / `PartialEq` 是给外部业务仓用的：它们在自己的 `main.rs` 里
/// 写 `const MY_DOMAINS: &[DomainMount]`，合并时需要 `Clone`，
/// 测试里比较需要 `Debug` + `PartialEq`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainMount {
    pub path: &'static str,
    pub guard: MountGuard,
    pub routers: &'static [fn() -> Router],
}

/// 基座内置域挂载登记表（装配点收敛）：新增平台域在此追加一行，
/// `infra/router.rs` 据此循环挂载，不再逐域手写 push + 中间件三件套。
/// 顺序贴合历史挂载顺序，便于 diff 对照。
///
/// **这不是全部**：外部业务仓库注册的域不在本表内，见 [`all_domains`]。
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

// ---------------------------------------------------------------------------
// 外部业务域注册（2026-09-18 新增）
// ---------------------------------------------------------------------------
//
// # 背景：为什么需要这个
//
// 在只交付一个二进制的年代，业务域跟平台域同处一仓，`DOMAINS` 直接改就行。
// 但业务仓库（如 tide-hr）现在以 **git 依赖 + 版本 tag** 消费本 crate——
// 它们改不了基座的源码，因此需要一个**运行时往装配表里追加行**的入口。
//
// # 设计取舍：为什么不用全局可变静态量
//
// 最直觉的做法是 `static EXTRA: Mutex<Vec<DomainMount>>` + `add_domain()`。
// 那需要把 `DomainMount` 的字段改成 `'static` 之外的形态（或要求调用方
// `Box::leak`），并且引入了「主线程先注册、否则漏挂」的隐式时序依赖，
// 在测试并行时会变成随机失败。
//
// 本模块改用**显式传递**：路由组装收一个 `&[DomainMount]`。
// - 基座自己的二进制（`main.rs`）不需要注册，行为与改造前完全一致；
// - 业务仓库在自己的 `main.rs` 里写一个 `const MY_DOMAINS: &[DomainMount]`，
//   然后把 `extra` 传给 `router::build_with`；
// - 没有隐藏状态，测试可以各自传各自的行，并行安全。
//
// # 业务仓库怎么用
//
// ```ignore
// // tide-hr/src/main.rs
// use salvo::prelude::Router;
// use tide_server::modules::{DomainMount, MountGuard};
//
// /// 本仓库自己的业务域：写法与基座内置行完全一致。
// const HR_DOMAINS: &[DomainMount] = &[DomainMount {
//     path: "employee",
//     guard: MountGuard::Protected,
//     routers: &[hr_employee::routes],
// }];
//
// #[tokio::main]
// async fn main() -> anyhow::Result<()> {
//     let config = tide_server::infra::config::Config::load()?;
//     tide_server::infra::app::run_with_domains(config, HR_DOMAINS).await
// }
// ```
//
// 业务仓的域同样得到 `AuthRequired` / `OperationLog` / `ApiPermission`
// 三件套（选 `MountGuard::Protected` 时），**无需自己接鉴权**；
// 别忘了把新接口登记进 `sys_api`（未登记接口按 fail-open 放行，见根 README）。

/// 合并基座内置域与外部注册域，得到最终挂载顺序。
///
/// 顺序 = [`DOMAINS`] 在前、`extra` 在后。路由匹配是「先注册先命中」吗？
/// 不是——Salvo 的路由树按完整路径匹配，基座内置的 19 条与业务域不同前缀，
/// 不存在覆盖关系；但**同前缀**的两种情况要留意：
/// - 业务域想复用一个**已存在的**前缀（如往 `/user` 下加端点）：
///   应当改基座（属于平台能力），或在业务域用**自己的**前缀；
/// - 业务域之间同前缀：由业务仓自己保证前缀唯一。
pub fn all_domains(extra: &[DomainMount]) -> Vec<DomainMount> {
    let mut all = Vec::with_capacity(DOMAINS.len() + extra.len());
    all.extend_from_slice(DOMAINS);
    all.extend_from_slice(extra);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvo::prelude::Router;

    /// 业务域出口：模拟外部仓库提供的 `routes()`。
    fn hr_employee_routes() -> Router {
        Router::with_path("list").goal(salvo::handler::empty())
    }

    /// **C3 扩展点回归**（2026-09-18）：外部业务仓能把自己的域追加到装配表，
    /// 且基座内置的 19 条不被覆盖、顺序仍在前面。
    ///
    /// 改造前不可能写出本用例：`DOMAINS` 是 `const`，路由组装直接读它，
    /// 外部 crate 没有追加入口。
    #[test]
    fn external_domains_are_appended_after_builtin() {
        const EXTRA: &[DomainMount] = &[DomainMount {
            path: "employee",
            guard: MountGuard::Protected,
            routers: &[hr_employee_routes],
        }];

        let merged = all_domains(EXTRA);

        // 内置域一条不少、且都在前面（C4：行为不变）。
        assert_eq!(merged.len(), DOMAINS.len() + 1);
        assert_eq!(&merged[..DOMAINS.len()], DOMAINS);

        // 外部域确实在尾部，且字段未被改写。
        let last = merged.last().expect("合并结果不应为空");
        assert_eq!(last.path, "employee");
        assert_eq!(last.guard, MountGuard::Protected);
        assert_eq!(last.routers.len(), 1);
    }

    /// 业务仓不注册任何域时与改造前完全一致（`build` 走的正是这条路径）。
    #[test]
    fn empty_extra_yields_builtin_only() {
        let merged = all_domains(&[]);
        assert_eq!(merged.len(), DOMAINS.len());
        assert!(merged.iter().eq(DOMAINS.iter()));
    }
}
