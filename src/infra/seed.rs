//! 开发环境种子数据：启动时幂等执行（可重复运行，不重复插入）。
//!
//! 内容：admin 用户、super 角色、默认菜单树（目录/页面/按钮权限码）、
//! admin-super 绑定、super 全量菜单绑定。
//! ⚠️ 开发约定：admin 密码固定为 `admin123`（每次启动重置）；生产环境请勿挂载本初始化。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;

use crate::entity::{
    hr_time_off_type, sys_api, sys_dictionary, sys_dictionary_detail, sys_job, sys_menu, sys_role,
    sys_role_menu, sys_user, sys_user_role,
};
use crate::utils::crypt;

/// admin 初始密码（开发环境约定）。
pub const SEED_ADMIN_PASSWORD: &str = "admin123";
/// admin 用户名。
pub const SEED_ADMIN_USERNAME: &str = "admin";
/// 超级管理员角色键（与 `permission::SUPER_ROLE_KEY` 同值，避免依赖方向循环）。
const SEED_SUPER_ROLE_KEY: &str = "super";

/// 接口登记路径规范化：与 `middleware::api_permission::canonical_path` 同一规则。
///
/// 判定面查 `sys_api` 前会把请求路径规范化（逐段 decode + 丢弃空段 + 去尾斜杠），
/// 故登记侧必须同步规范化，否则登记成含尾斜杠/重复斜杠/转义的形式会永远匹配不上，
/// 该接口会落回 fail-open。返回 `Err` 时该条登记跳过并记日志（不阻止启动）。
fn canonical_api_path(path: &str) -> anyhow::Result<String> {
    let normalized = crate::middleware::api_permission::canonical_path(path);
    anyhow::ensure!(
        normalized.starts_with("/api/"),
        "接口路径必须形如 /api/...，实际登记为 {path:?}"
    );
    Ok(normalized)
}

/// 播种互斥锁：ensure_seed 的"先查后插"在并发下不幂等（TOCTOU），
/// 启动初始化与测试并发调用时通过进程内锁串行化。
static SEED_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

/// 菜单种子定义：`parent` 引用父菜单的 `name`（顶层为 `None`）。
struct MenuSeed {
    name: &'static str,
    title: &'static str,
    path: &'static str,
    component: &'static str,
    icon: &'static str,
    menu_type: i8,
    permission: &'static str,
    parent: Option<&'static str>,
    sort: i32,
}

/// 默认菜单树（vben 契约：顶级 path 以 `/` 开头、component 可被 glob 命中、
/// name 全局唯一；menu_type：1 目录 / 2 页面 / 3 按钮）。
const MENU_SEEDS: &[MenuSeed] = &[
    // 系统管理目录
    MenuSeed {
        name: "System",
        title: "系统管理",
        path: "/system",
        component: "",
        icon: "lucide:settings",
        menu_type: 1,
        permission: "",
        parent: None,
        sort: 1,
    },
    // 用户管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemUser",
        title: "用户管理",
        path: "/system/user",
        component: "#/views/system/user/index.vue",
        icon: "lucide:users",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemUserCreate",
        title: "用户新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:user:create",
        parent: Some("SystemUser"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemUserUpdate",
        title: "用户修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:user:update",
        parent: Some("SystemUser"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemUserDelete",
        title: "用户删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:user:delete",
        parent: Some("SystemUser"),
        sort: 3,
    },
    // 部门管理页面 + 按钮权限码（组织架构树；sort 靠后，不打扰既有菜单顺序）
    MenuSeed {
        name: "SystemDept",
        title: "部门管理",
        path: "/system/dept",
        component: "#/views/system/dept/index.vue",
        icon: "lucide:network",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 9,
    },
    MenuSeed {
        name: "SystemDeptCreate",
        title: "部门新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dept:create",
        parent: Some("SystemDept"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemDeptUpdate",
        title: "部门修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dept:update",
        parent: Some("SystemDept"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemDeptDelete",
        title: "部门删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dept:delete",
        parent: Some("SystemDept"),
        sort: 3,
    },
    // 职位管理页面 + 按钮权限码（职务维度主数据；sort 靠后，不打扰既有菜单顺序）
    MenuSeed {
        name: "SystemPosition",
        title: "职位管理",
        path: "/system/position",
        component: "#/views/system/position/index.vue",
        icon: "lucide:briefcase",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 10,
    },
    MenuSeed {
        name: "SystemPositionCreate",
        title: "职位新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:position:create",
        parent: Some("SystemPosition"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemPositionUpdate",
        title: "职位修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:position:update",
        parent: Some("SystemPosition"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemPositionDelete",
        title: "职位删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:position:delete",
        parent: Some("SystemPosition"),
        sort: 3,
    },
    // 角色管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemRole",
        title: "角色管理",
        path: "/system/role",
        component: "#/views/system/role/index.vue",
        icon: "lucide:shield",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemRoleCreate",
        title: "角色新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:role:create",
        parent: Some("SystemRole"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemRoleUpdate",
        title: "角色修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:role:update",
        parent: Some("SystemRole"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemRoleDelete",
        title: "角色删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:role:delete",
        parent: Some("SystemRole"),
        sort: 3,
    },
    // 菜单管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemMenu",
        title: "菜单管理",
        path: "/system/menu",
        component: "#/views/system/menu/index.vue",
        icon: "lucide:menu",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 3,
    },
    MenuSeed {
        name: "SystemMenuCreate",
        title: "菜单新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:menu:create",
        parent: Some("SystemMenu"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemMenuUpdate",
        title: "菜单修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:menu:update",
        parent: Some("SystemMenu"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemMenuDelete",
        title: "菜单删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:menu:delete",
        parent: Some("SystemMenu"),
        sort: 3,
    },
    // API 管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemApi",
        title: "API 管理",
        path: "/system/api",
        component: "#/views/system/api/index.vue",
        icon: "lucide:webhook",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 4,
    },
    MenuSeed {
        name: "SystemApiCreate",
        title: "API 新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:api:create",
        parent: Some("SystemApi"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemApiUpdate",
        title: "API 修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:api:update",
        parent: Some("SystemApi"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemApiDelete",
        title: "API 删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:api:delete",
        parent: Some("SystemApi"),
        sort: 3,
    },
    // 操作日志页面 + 删除权限码
    MenuSeed {
        name: "SystemOperationLog",
        title: "操作日志",
        path: "/system/operation-log",
        component: "#/views/system/operation-log/index.vue",
        icon: "lucide:scroll-text",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 5,
    },
    MenuSeed {
        name: "SystemOperationLogDelete",
        title: "操作日志删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:operation-log:delete",
        parent: Some("SystemOperationLog"),
        sort: 1,
    },
    // 登录日志页面 + 删除权限码
    MenuSeed {
        name: "SystemLoginLog",
        title: "登录日志",
        path: "/system/login-log",
        component: "#/views/system/login-log/index.vue",
        icon: "lucide:history",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 6,
    },
    MenuSeed {
        name: "SystemLoginLogDelete",
        title: "登录日志删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:login-log:delete",
        parent: Some("SystemLoginLog"),
        sort: 1,
    },
    // 会话管理页面 + 强制下线 / 删除权限码（页面：#views/system/session/index.vue）
    MenuSeed {
        name: "SystemSession",
        title: "会话管理",
        path: "/system/session",
        component: "#/views/system/session/index.vue",
        icon: "lucide:activity",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 11,
    },
    MenuSeed {
        name: "SystemSessionForceLogout",
        title: "会话强制下线",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:session:force-logout",
        parent: Some("SystemSession"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemSessionDelete",
        title: "会话删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:session:delete",
        parent: Some("SystemSession"),
        sort: 2,
    },
    // 数据字典页面 + 类型 / 字典项各三个按钮权限码
    MenuSeed {
        name: "SystemDictionary",
        title: "数据字典",
        path: "/system/dictionary",
        component: "#/views/system/dictionary/index.vue",
        icon: "lucide:book-marked",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 7,
    },
    MenuSeed {
        name: "SystemDictionaryCreate",
        title: "字典类型新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary:create",
        parent: Some("SystemDictionary"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemDictionaryUpdate",
        title: "字典类型编辑",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary:update",
        parent: Some("SystemDictionary"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemDictionaryDelete",
        title: "字典类型删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary:delete",
        parent: Some("SystemDictionary"),
        sort: 3,
    },
    MenuSeed {
        name: "SystemDictionaryDetailCreate",
        title: "字典项新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary-detail:create",
        parent: Some("SystemDictionary"),
        sort: 4,
    },
    MenuSeed {
        name: "SystemDictionaryDetailUpdate",
        title: "字典项编辑",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary-detail:update",
        parent: Some("SystemDictionary"),
        sort: 5,
    },
    MenuSeed {
        name: "SystemDictionaryDetailDelete",
        title: "字典项删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary-detail:delete",
        parent: Some("SystemDictionary"),
        sort: 6,
    },
    // 定时任务页面 + 按钮权限码
    MenuSeed {
        name: "SystemJob",
        title: "定时任务",
        path: "/system/job",
        component: "#/views/system/job/index.vue",
        icon: "lucide:clock",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 8,
    },
    MenuSeed {
        name: "SystemJobCreate",
        title: "任务新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:job:create",
        parent: Some("SystemJob"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemJobUpdate",
        title: "任务编辑",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:job:update",
        parent: Some("SystemJob"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemJobDelete",
        title: "任务删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:job:delete",
        parent: Some("SystemJob"),
        sort: 3,
    },
    MenuSeed {
        name: "SystemJobUpdateStatus",
        title: "任务启停",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:job:update-status",
        parent: Some("SystemJob"),
        sort: 4,
    },
    MenuSeed {
        name: "SystemJobRunOnce",
        title: "任务立即执行",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:job:run-once",
        parent: Some("SystemJob"),
        sort: 5,
    },
    MenuSeed {
        name: "SystemJobLogDelete",
        title: "执行日志删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:job-log:delete",
        parent: Some("SystemJob"),
        sort: 6,
    },
    // 人事管理目录（业务域 `biz/hr`）：员工档案页 + 按钮权限码。
    // 按钮码只控前端显隐，后端判定走 sys_api（见 API_SEEDS 的 hr/employee 段）。
    MenuSeed {
        name: "Hr",
        title: "人事管理",
        path: "/hr",
        component: "",
        icon: "lucide:users",
        menu_type: 1,
        permission: "",
        parent: None,
        sort: 2,
    },
    MenuSeed {
        name: "HrEmployee",
        title: "员工档案",
        path: "/hr/employee",
        component: "#/views/biz/hr/employee/index.vue",
        icon: "lucide:id-card",
        menu_type: 2,
        permission: "",
        parent: Some("Hr"),
        sort: 1,
    },
    MenuSeed {
        name: "HrEmployeeCreate",
        title: "员工新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:employee:create",
        parent: Some("HrEmployee"),
        sort: 1,
    },
    MenuSeed {
        name: "HrEmployeeUpdate",
        title: "员工修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:employee:update",
        parent: Some("HrEmployee"),
        sort: 2,
    },
    MenuSeed {
        name: "HrEmployeeDelete",
        title: "员工删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:employee:delete",
        parent: Some("HrEmployee"),
        sort: 3,
    },
    // 假期额度（业务域 `biz/hr/time_off`）：假期管理目录 + 三个页面 + 按钮权限码。
    MenuSeed {
        name: "HrTimeOff",
        title: "假期管理",
        path: "/hr/time-off",
        component: "",
        icon: "lucide:calendar-days",
        menu_type: 1,
        permission: "",
        parent: Some("Hr"),
        sort: 2,
    },
    MenuSeed {
        name: "HrTimeOffType",
        title: "假期类型",
        path: "/hr/time-off/type",
        component: "#/views/biz/hr/time-off/type/index.vue",
        icon: "lucide:tags",
        menu_type: 2,
        permission: "",
        parent: Some("HrTimeOff"),
        sort: 1,
    },
    MenuSeed {
        name: "HrTimeOffTypeCreate",
        title: "类型新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:time-off-type:create",
        parent: Some("HrTimeOffType"),
        sort: 1,
    },
    MenuSeed {
        name: "HrTimeOffTypeUpdate",
        title: "类型修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:time-off-type:update",
        parent: Some("HrTimeOffType"),
        sort: 2,
    },
    MenuSeed {
        name: "HrTimeOffTypeDelete",
        title: "类型删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:time-off-type:delete",
        parent: Some("HrTimeOffType"),
        sort: 3,
    },
    MenuSeed {
        name: "HrTimeOffGrant",
        title: "额度发放",
        path: "/hr/time-off/grant",
        component: "#/views/biz/hr/time-off/grant/index.vue",
        icon: "lucide:gift",
        menu_type: 2,
        permission: "",
        parent: Some("HrTimeOff"),
        sort: 2,
    },
    MenuSeed {
        name: "HrTimeOffGrantCreate",
        title: "发放",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:time-off-grant:create",
        parent: Some("HrTimeOffGrant"),
        sort: 1,
    },
    MenuSeed {
        name: "HrTimeOffGrantCancel",
        title: "撤销发放",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "hr:time-off-grant:cancel",
        parent: Some("HrTimeOffGrant"),
        sort: 2,
    },
    MenuSeed {
        name: "HrTimeOffBalance",
        title: "额度查询",
        path: "/hr/time-off/balance",
        component: "#/views/biz/hr/time-off/balance/index.vue",
        icon: "lucide:wallet",
        menu_type: 2,
        permission: "",
        parent: Some("HrTimeOff"),
        sort: 3,
    },
];

/// API 种子定义：`path` 必须带 `/api/v1` 前缀且**写成规范化形式**（无尾斜杠、无重复
/// 斜杠、无转义），与 `middleware::api_permission::canonical_path` 的规范化结果
/// 一致——判定面查 `sys_api` 前会先规范化请求路径，登记侧写法必须与之对齐；
/// `method` 用大写。
struct ApiSeed {
    path: &'static str,
    method: &'static str,
    description: &'static str,
    api_group: &'static str,
}

const fn api(
    path: &'static str,
    method: &'static str,
    description: &'static str,
    api_group: &'static str,
) -> ApiSeed {
    ApiSeed {
        path,
        method,
        description,
        api_group,
    }
}

/// 全部管理端点登记（92 条）。刻意排除：
/// - 公开接口：/health、/captcha/generate、/auth/{login,logout}、GET /site-config/get；
/// - 登录后每个用户必调的契约端点：POST /user/{info,access-codes,menus}
///   （登记即 fail-closed，会把所有非超管用户挡在登录态之外）。
const API_SEEDS: &[ApiSeed] = &[
    // 用户管理（POST /api/v1/user/*，排除 info / access-codes 契约端点）
    api("/api/v1/user/list", "POST", "用户列表查询", "用户管理"),
    api(
        "/api/v1/user/by-username",
        "POST",
        "按用户名查询用户",
        "用户管理",
    ),
    api("/api/v1/user/create", "POST", "用户新增", "用户管理"),
    api("/api/v1/user/update", "POST", "用户修改", "用户管理"),
    api("/api/v1/user/get", "POST", "用户详情", "用户管理"),
    api("/api/v1/user/update-status", "POST", "用户启停", "用户管理"),
    api("/api/v1/user/delete", "POST", "用户删除", "用户管理"),
    api("/api/v1/user/get-depts", "POST", "用户部门列表", "用户管理"),
    api(
        "/api/v1/user/get-positions",
        "POST",
        "用户职位列表",
        "用户管理",
    ),
    api("/api/v1/user/list-all", "POST", "全量用户列表", "用户管理"),
    api(
        "/api/v1/user/list-all-includes-soft-deleted",
        "POST",
        "全量用户列表（含软删，审计筛选）",
        "用户管理",
    ),
    // 角色管理
    api("/api/v1/role/list", "POST", "角色列表查询", "角色管理"),
    api("/api/v1/role/create", "POST", "角色新增", "角色管理"),
    api("/api/v1/role/update", "POST", "角色修改", "角色管理"),
    api("/api/v1/role/get", "POST", "角色详情", "角色管理"),
    api("/api/v1/role/delete", "POST", "角色删除", "角色管理"),
    api("/api/v1/role/update-status", "POST", "角色启停", "角色管理"),
    api("/api/v1/role/list-all", "POST", "全量角色列表", "角色管理"),
    api(
        "/api/v1/role/list-all-enabled",
        "POST",
        "启用角色列表",
        "角色管理",
    ),
    // 菜单管理
    api("/api/v1/menu/list", "POST", "菜单列表查询", "菜单管理"),
    api("/api/v1/menu/create", "POST", "菜单新增", "菜单管理"),
    api("/api/v1/menu/update", "POST", "菜单修改", "菜单管理"),
    api("/api/v1/menu/get", "POST", "菜单详情", "菜单管理"),
    api("/api/v1/menu/delete", "POST", "菜单删除", "菜单管理"),
    // 部门管理
    api("/api/v1/dept/list", "POST", "部门树列表查询", "部门管理"),
    api("/api/v1/dept/create", "POST", "部门新增", "部门管理"),
    api("/api/v1/dept/update", "POST", "部门修改", "部门管理"),
    api("/api/v1/dept/get", "POST", "部门详情", "部门管理"),
    api("/api/v1/dept/delete", "POST", "部门删除", "部门管理"),
    // 数据字典（类型 + 字典项）
    api(
        "/api/v1/dictionary/list",
        "POST",
        "字典类型列表查询",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary/create",
        "POST",
        "字典类型新增",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary/update",
        "POST",
        "字典类型修改",
        "数据字典",
    ),
    api("/api/v1/dictionary/get", "POST", "字典类型详情", "数据字典"),
    api(
        "/api/v1/dictionary/delete",
        "POST",
        "字典类型删除",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary/get-by-type",
        "POST",
        "按类型查询字典项（通用）",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary-detail/list",
        "POST",
        "字典项列表查询",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary-detail/create",
        "POST",
        "字典项新增",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary-detail/update",
        "POST",
        "字典项修改",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary-detail/get",
        "POST",
        "字典项详情",
        "数据字典",
    ),
    api(
        "/api/v1/dictionary-detail/delete",
        "POST",
        "字典项删除",
        "数据字典",
    ),
    // 接口管理
    api("/api/v1/sys-api/list", "POST", "API 列表查询", "接口管理"),
    api("/api/v1/sys-api/create", "POST", "API 新增", "接口管理"),
    api("/api/v1/sys-api/update", "POST", "API 修改", "接口管理"),
    api("/api/v1/sys-api/get", "POST", "API 详情", "接口管理"),
    api("/api/v1/sys-api/delete", "POST", "API 删除", "接口管理"),
    api(
        "/api/v1/sys-api/list-all",
        "POST",
        "全量 API 列表",
        "接口管理",
    ),
    // 操作日志
    api(
        "/api/v1/operation-log/list",
        "POST",
        "操作日志列表查询",
        "操作日志",
    ),
    api(
        "/api/v1/operation-log/get",
        "POST",
        "操作日志详情",
        "操作日志",
    ),
    api(
        "/api/v1/operation-log/delete",
        "POST",
        "操作日志删除",
        "操作日志",
    ),
    api(
        "/api/v1/operation-log/delete-batch",
        "POST",
        "操作日志批量删除",
        "操作日志",
    ),
    // 登录日志
    api(
        "/api/v1/login-log/list",
        "POST",
        "登录日志列表查询",
        "登录日志",
    ),
    api("/api/v1/login-log/get", "POST", "登录日志详情", "登录日志"),
    api(
        "/api/v1/login-log/delete",
        "POST",
        "登录日志删除",
        "登录日志",
    ),
    api(
        "/api/v1/login-log/delete-batch",
        "POST",
        "登录日志批量删除",
        "登录日志",
    ),
    // 文件管理（download 为 GET）
    api("/api/v1/file/list", "POST", "文件列表查询", "文件管理"),
    api("/api/v1/file/upload", "POST", "文件上传", "文件管理"),
    api("/api/v1/file/get", "POST", "文件详情", "文件管理"),
    api("/api/v1/file/download", "GET", "文件下载", "文件管理"),
    api("/api/v1/file/delete", "POST", "文件删除", "文件管理"),
    // 参数配置
    api("/api/v1/config/list", "POST", "参数列表查询", "参数配置"),
    api("/api/v1/config/create", "POST", "参数新增", "参数配置"),
    api("/api/v1/config/update", "POST", "参数修改", "参数配置"),
    api("/api/v1/config/get", "POST", "参数详情", "参数配置"),
    api("/api/v1/config/delete", "POST", "参数删除", "参数配置"),
    // 网站设置（GET get 为公开接口，不登记）
    api(
        "/api/v1/site-config/update",
        "POST",
        "网站设置更新",
        "网站设置",
    ),
    // 定时任务
    api("/api/v1/job/list", "POST", "任务列表查询", "定时任务"),
    api("/api/v1/job/create", "POST", "任务新增", "定时任务"),
    api("/api/v1/job/update", "POST", "任务修改", "定时任务"),
    api("/api/v1/job/get", "POST", "任务详情", "定时任务"),
    api("/api/v1/job/delete", "POST", "任务删除", "定时任务"),
    api("/api/v1/job/update-status", "POST", "任务启停", "定时任务"),
    api("/api/v1/job/run-once", "POST", "任务立即执行", "定时任务"),
    api("/api/v1/job/handlers", "POST", "任务处理器列表", "定时任务"),
    // 任务日志
    api(
        "/api/v1/job-log/list",
        "POST",
        "执行日志列表查询",
        "任务日志",
    ),
    api("/api/v1/job-log/get", "POST", "执行日志详情", "任务日志"),
    api("/api/v1/job-log/delete", "POST", "执行日志删除", "任务日志"),
    api(
        "/api/v1/job-log/delete-batch",
        "POST",
        "执行日志批量删除",
        "任务日志",
    ),
    // 职位管理
    api("/api/v1/position/list", "POST", "职位列表查询", "职位管理"),
    api("/api/v1/position/create", "POST", "职位新增", "职位管理"),
    api("/api/v1/position/update", "POST", "职位修改", "职位管理"),
    api("/api/v1/position/get", "POST", "职位详情", "职位管理"),
    api("/api/v1/position/delete", "POST", "职位删除", "职位管理"),
    // 刷新凭证（/auth/refresh 属公开契约端点，不登记）
    api(
        "/api/v1/refresh-token/list",
        "POST",
        "刷新凭证列表查询",
        "刷新凭证",
    ),
    api(
        "/api/v1/refresh-token/delete",
        "POST",
        "刷新凭证删除（仅历史记录）",
        "刷新凭证",
    ),
    api(
        "/api/v1/refresh-token/delete-batch",
        "POST",
        "刷新凭证批量删除（仅历史记录）",
        "刷新凭证",
    ),
    api(
        "/api/v1/refresh-token/force-logout",
        "POST",
        "会话强制下线",
        "刷新凭证",
    ),
    api(
        "/api/v1/refresh-token/force-logout-user",
        "POST",
        "按用户强制下线全部会话",
        "刷新凭证",
    ),
    // 员工档案（业务域 hr/employee）：与 DOMAINS 的 "hr/employee" 档位、菜单
    // HrEmployee 及其按钮码一一对应；漏登 = 该端点 fail-open（未登记放行）。
    api(
        "/api/v1/hr/employee/list",
        "POST",
        "员工档案列表查询",
        "员工档案",
    ),
    api(
        "/api/v1/hr/employee/create",
        "POST",
        "员工档案新增",
        "员工档案",
    ),
    api(
        "/api/v1/hr/employee/update",
        "POST",
        "员工档案修改",
        "员工档案",
    ),
    api(
        "/api/v1/hr/employee/get",
        "POST",
        "员工档案详情",
        "员工档案",
    ),
    api(
        "/api/v1/hr/employee/delete",
        "POST",
        "员工档案删除",
        "员工档案",
    ),
    // 假期额度（业务域 hr/time-off）：与 DOMAINS 的 "hr/time-off" 档位、菜单 HrTimeOff* 一一对应；
    // 漏登 = 该端点 fail-open（未登记放行）。
    api(
        "/api/v1/hr/time-off/type/list",
        "POST",
        "假期类型列表查询",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/type/create",
        "POST",
        "假期类型新增",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/type/update",
        "POST",
        "假期类型修改",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/type/get",
        "POST",
        "假期类型详情",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/type/delete",
        "POST",
        "假期类型删除",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/grant/list",
        "POST",
        "额度批次列表查询",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/grant/batch-create",
        "POST",
        "额度批量发放",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/grant/get",
        "POST",
        "额度批次详情",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/grant/cancel",
        "POST",
        "额度批次撤销",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/balance/list",
        "POST",
        "额度账户列表查询",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/balance/get",
        "POST",
        "额度账户详情",
        "假期额度",
    ),
    api(
        "/api/v1/hr/time-off/balance/logs",
        "POST",
        "额度流水查询",
        "假期额度",
    ),
];

/// 按 name 查菜单 id（不过滤软删，与 seed 的查重口径一致）。
async fn find_menu_id_by_name(db: &DatabaseConnection, name: &str) -> anyhow::Result<Option<u64>> {
    let menu = sys_menu::Entity::find()
        .filter(sys_menu::Column::Name.eq(name))
        .one(db)
        .await?;
    Ok(menu.map(|menu| menu.id))
}

/// 播种一个「整数值」字典类型及其字典项，返回字典类型 id（幂等：类型按 `type`
/// 查重、字典项按 `dictionary_id + value` 查重；只补缺、不覆盖运维改过的行）。
///
/// 供业务域枚举使用（人事的 `employmentStatus` / `education`）。字符串枚举
/// （如 `timeOffGrantReason`）亦可复用本函数：`sys_dictionary_detail.value` 本就是
/// 字符串列，`items` 的 `value` 写 `statutory` 这类编码即可。平台既有的
/// `status` / `execResultStatus` 保留各自的历史迁移逻辑（旧编码原地改名、extend
/// 回填），刻意不走本函数，改动前先读那两段注释。
async fn seed_int_dictionary(
    db: &DatabaseConnection,
    admin_id: u64,
    dict_type: &str,
    name: &str,
    remark: &str,
    items: &[(&str, &str, i32)],
) -> anyhow::Result<u64> {
    let dict_id = match sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(dict_type))
        .one(db)
        .await?
    {
        Some(dict) => dict.id,
        None => {
            sys_dictionary::ActiveModel {
                name: Set(name.to_string()),
                r#type: Set(dict_type.to_string()),
                status: Set(1),
                remark: Set(remark.to_string()),
                // 种子数据的操作人统一记为 admin 自己
                created_by: Set(admin_id),
                updated_by: Set(admin_id),
                ..Default::default()
            }
            .insert(db)
            .await?
            .id
        }
    };

    for (label, value, sort) in items {
        let existing = sys_dictionary_detail::Entity::find()
            .filter(sys_dictionary_detail::Column::DictionaryId.eq(dict_id))
            .filter(sys_dictionary_detail::Column::Value.eq(*value))
            .one(db)
            .await?;
        if existing.is_none() {
            sys_dictionary_detail::ActiveModel {
                dictionary_id: Set(dict_id),
                label: Set(String::from(*label)),
                value: Set(String::from(*value)),
                extend: Set(String::new()),
                sort: Set(*sort),
                status: Set(1),
                created_by: Set(admin_id),
                updated_by: Set(admin_id),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }

    Ok(dict_id)
}

/// 启动初始化：确保开发种子数据存在（幂等，可重复调用）。
pub async fn ensure_seed(db: &DatabaseConnection) -> anyhow::Result<()> {
    let lock = SEED_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;

    // 1. admin 用户：存在则重置密码（开发约定），不存在则创建
    let admin = sys_user::Entity::find()
        .filter(sys_user::Column::Username.eq(SEED_ADMIN_USERNAME))
        .one(db)
        .await?;
    let password_hash = crypt::hash_password(SEED_ADMIN_PASSWORD)?;
    let admin_id = if let Some(admin) = admin {
        let mut admin: sys_user::ActiveModel = admin.into();
        admin.password = Set(password_hash);
        admin.update(db).await?.id
    } else {
        let inserted = sys_user::ActiveModel {
            username: Set(SEED_ADMIN_USERNAME.to_string()),
            password: Set(password_hash),
            emp_no: Set("admin".to_string()),
            nickname: Set("超级管理员".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await?;
        let admin_id = inserted.id;
        // 种子数据的操作人统一记为 admin 自己：自增 id 非 1 时回填真实 id，
        // 避免 created_by 指向不存在的用户。
        let mut audit: sys_user::ActiveModel = inserted.into();
        audit.created_by = Set(admin_id);
        audit.updated_by = Set(admin_id);
        audit.update(db).await?;
        admin_id
    };

    // 2. super 角色：不存在则创建
    let super_role = sys_role::Entity::find()
        .filter(sys_role::Column::RoleKey.eq(SEED_SUPER_ROLE_KEY))
        .one(db)
        .await?;
    let super_role_id = if let Some(role) = super_role {
        role.id
    } else {
        sys_role::ActiveModel {
            role_name: Set("超级管理员".to_string()),
            role_key: Set(SEED_SUPER_ROLE_KEY.to_string()),
            sort: Set(0),
            status: Set(1),
            remark: Set("系统内置超管角色".to_string()),
            // 种子数据的操作人统一记为 admin 自己
            created_by: Set(admin_id),
            updated_by: Set(admin_id),
            ..Default::default()
        }
        .insert(db)
        .await?
        .id
    };

    // 3. admin-super 绑定：不存在则创建
    let bound = sys_user_role::Entity::find()
        .filter(sys_user_role::Column::UserId.eq(admin_id))
        .filter(sys_user_role::Column::RoleId.eq(super_role_id))
        .one(db)
        .await?;
    if bound.is_none() {
        sys_user_role::ActiveModel {
            user_id: Set(admin_id),
            role_id: Set(super_role_id),
        }
        .insert(db)
        .await?;
    }

    // 4. 默认菜单：按 name 逐个补种，并记录 name → id 映射（按钮的 parent 引用）。
    //    并发安全：`uk_sys_menu_name` 唯一索引兜底，插入冲突时回查既有行（另一进程
    //    已插同批种子），避免「先查后插」在多进程下重复插入（2026-09-11 实际踩到）。
    let mut menu_ids_by_name: std::collections::HashMap<&str, u64> = Default::default();
    for seed in MENU_SEEDS {
        let existing = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq(seed.name))
            .one(db)
            .await?;
        let menu_id = if let Some(menu) = existing {
            menu.id
        } else {
            // 父 id：优先本批已解析的映射；父由并发进程插入时回查数据库兜底
            let parent_id = match seed.parent {
                Some(parent_name) => match menu_ids_by_name.get(parent_name) {
                    Some(id) => *id,
                    None => find_menu_id_by_name(db, parent_name).await?.unwrap_or(0),
                },
                None => 0,
            };
            let insert_result = sys_menu::ActiveModel {
                parent_id: Set(parent_id),
                path: Set(seed.path.to_string()),
                name: Set(seed.name.to_string()),
                component: Set(seed.component.to_string()),
                title: Set(seed.title.to_string()),
                icon: Set(seed.icon.to_string()),
                sort: Set(seed.sort),
                keep_alive: Set(0),
                hidden: Set(0),
                menu_type: Set(seed.menu_type),
                permission: Set(seed.permission.to_string()),
                status: Set(1),
                // 种子数据的操作人统一记为 admin 自己
                created_by: Set(admin_id),
                updated_by: Set(admin_id),
                ..Default::default()
            }
            .insert(db)
            .await;
            match insert_result {
                Ok(menu) => menu.id,
                // 唯一索引冲突（同一 name 刚被并发进程插入）：回查既有 id，不中断 seed
                Err(_) => find_menu_id_by_name(db, seed.name)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("菜单种子插入失败且回查不到：{}", seed.name))?,
            }
        };
        menu_ids_by_name.insert(seed.name, menu_id);
    }

    // 4.1 数据字典：通用启用状态（type=status）。用户/角色等 status 字段校验的取值
    //     来源（校验侧见 modules::dictionary::service::enabled_int_values）。
    const SEED_DICT_TYPE_STATUS: &str = "status";
    let status_dict = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(SEED_DICT_TYPE_STATUS))
        .one(db)
        .await?;
    let status_dict_id = if let Some(d) = status_dict {
        d.id
    } else {
        sys_dictionary::ActiveModel {
            name: Set("启用状态".to_string()),
            r#type: Set(SEED_DICT_TYPE_STATUS.to_string()),
            status: Set(1),
            remark: Set(
                "通用启用/禁用状态（1 启用、0 禁用），作为各表 status 字段校验的数据字典"
                    .to_string(),
            ),
            created_by: Set(admin_id),
            updated_by: Set(admin_id),
            ..Default::default()
        }
        .insert(db)
        .await?
        .id
    };
    let status_items: &[(&str, &str, i32)] = &[("启用", "1", 1), ("禁用", "0", 2)];
    for (label, value, sort) in status_items {
        let existing = sys_dictionary_detail::Entity::find()
            .filter(sys_dictionary_detail::Column::DictionaryId.eq(status_dict_id))
            .filter(sys_dictionary_detail::Column::Value.eq(*value))
            .one(db)
            .await?;
        if existing.is_none() {
            sys_dictionary_detail::ActiveModel {
                dictionary_id: Set(status_dict_id),
                label: Set(String::from(*label)),
                value: Set(String::from(*value)),
                extend: Set(String::new()),
                sort: Set(*sort),
                status: Set(1),
                created_by: Set(admin_id),
                updated_by: Set(admin_id),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }

    // 4.2 数据字典：执行结果状态（type=execResultStatus）。sys_job_log.status 与
    //     sys_login_log.status 共用展示/取值来源（1 成功、0 失败），与 sys_job.status 的
    //     「启用/禁用」（通用 type=status）无关。前端 job 日志 / 登录日志的 Tag 颜色
    //     直接取字典项 extend（作为 ElTag type），故两项固定写 success / danger。
    //     历史版本以 type=jobLogStatus 命名且仅覆盖任务日志：若旧行仍在且新编码尚未建立，
    //     则原地把 type 改为新编码并泛化 name/remark，保留 id、审计字段与已挂字典项，
    //     让新旧环境收敛到同一编码。
    const SEED_DICT_TYPE_EXEC_RESULT_STATUS: &str = "execResultStatus";
    const LEGACY_DICT_TYPE_JOB_LOG_STATUS: &str = "jobLogStatus";
    let exec_result_status_dict = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(SEED_DICT_TYPE_EXEC_RESULT_STATUS))
        .one(db)
        .await?;
    let exec_result_dict_id = if let Some(d) = exec_result_status_dict {
        d.id
    } else if let Some(d) = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(LEGACY_DICT_TYPE_JOB_LOG_STATUS))
        .one(db)
        .await?
    {
        // 旧命名迁移：type 改新编码、name/remark 泛化到登录 + 任务双场景；字典项不动
        let mut active: sys_dictionary::ActiveModel = d.into();
        active.r#type = Set(SEED_DICT_TYPE_EXEC_RESULT_STATUS.to_string());
        active.name = Set("执行结果状态".to_string());
        active.remark = Set(
            "登录日志（sys_login_log）与任务日志（sys_job_log）共用的执行结果字典（1 成功、0 失败）"
                .to_string(),
        );
        active.updated_by = Set(admin_id);
        active.update(db).await?.id
    } else {
        sys_dictionary::ActiveModel {
            name: Set("执行结果状态".to_string()),
            r#type: Set(SEED_DICT_TYPE_EXEC_RESULT_STATUS.to_string()),
            status: Set(1),
            remark: Set(
                "登录日志（sys_login_log）与任务日志（sys_job_log）共用的执行结果字典（1 成功、0 失败）"
                    .to_string(),
            ),
            created_by: Set(admin_id),
            updated_by: Set(admin_id),
            ..Default::default()
        }
        .insert(db)
        .await?
        .id
    };
    let exec_result_status_items: &[(&str, &str, &str, i32)] =
        &[("成功", "1", "success", 1), ("失败", "0", "danger", 2)];
    for (label, value, extend, sort) in exec_result_status_items {
        let existing = sys_dictionary_detail::Entity::find()
            .filter(sys_dictionary_detail::Column::DictionaryId.eq(exec_result_dict_id))
            .filter(sys_dictionary_detail::Column::Value.eq(*value))
            .one(db)
            .await?;
        match existing {
            None => {
                sys_dictionary_detail::ActiveModel {
                    dictionary_id: Set(exec_result_dict_id),
                    label: Set(String::from(*label)),
                    value: Set(String::from(*value)),
                    extend: Set(String::from(*extend)),
                    sort: Set(*sort),
                    status: Set(1),
                    created_by: Set(admin_id),
                    updated_by: Set(admin_id),
                    ..Default::default()
                }
                .insert(db)
                .await?;
            }
            // 兼容历史脏数据：旧 seed / 旧字典搬迁写入的字典项 extend 为空串，原地补写
            // 默认颜色（字典管理页可改，此处仅在空串时回填，非空保留运维自定义）。
            Some(item) if item.extend.is_empty() => {
                let mut active: sys_dictionary_detail::ActiveModel = item.into();
                active.extend = Set(String::from(*extend));
                active.updated_by = Set(admin_id);
                active.update(db).await?;
            }
            Some(_) => {}
        }
    }

    // 4.3 数据字典：人事业务枚举（type=employmentStatus / education）。员工档案
    //     （hr_employee.employment_status / education）的值域来源，接口层用
    //     dictionary::service::enabled_int_values 预取后交 validate（见
    //     modules::biz::hr::employee::api）。值改这里、不同步改代码即生效。
    const SEED_DICT_TYPE_EMPLOYMENT_STATUS: &str = "employmentStatus";
    const SEED_DICT_TYPE_EDUCATION: &str = "education";
    seed_int_dictionary(
        db,
        admin_id,
        SEED_DICT_TYPE_EMPLOYMENT_STATUS,
        "在职状态",
        "员工档案（hr_employee）在职状态：1 在职、2 试用、3 离职",
        &[("在职", "1", 1), ("试用", "2", 2), ("离职", "3", 3)],
    )
    .await?;
    seed_int_dictionary(
        db,
        admin_id,
        SEED_DICT_TYPE_EDUCATION,
        "最高学历",
        "员工档案（hr_employee）最高学历：0 未填、1 高中、2 大专、3 本科、4 硕士、5 博士",
        &[
            ("未填", "0", 1),
            ("高中", "1", 2),
            ("大专", "2", 3),
            ("本科", "3", 4),
            ("硕士", "4", 5),
            ("博士", "5", 6),
        ],
    )
    .await?;

    // 4.4 数据字典：假期发放依据（type=timeOffGrantReason，字符串枚举）。
    //     `hr_time_off_grant.reason` 的展示与取值来源（校验侧只查非空，不查值域）。
    const SEED_DICT_TYPE_LEAVE_GRANT_REASON: &str = "timeOffGrantReason";
    seed_int_dictionary(
        db,
        admin_id,
        SEED_DICT_TYPE_LEAVE_GRANT_REASON,
        "发放依据",
        "假期额度发放依据（hr_time_off_grant.reason）：法定年假 / 公司福利年假 / 司龄增补 / 上年结转 / 加班转调休 / 手工调整 / 离职补偿",
        &[
            ("法定年假", "statutory", 1),
            ("公司福利年假", "company", 2),
            ("司龄增补", "seniority", 3),
            ("上年结转", "carry", 4),
            ("加班转调休", "comp", 5),
            ("手工调整", "manual", 6),
            ("离职补偿", "severance", 7),
        ],
    )
    .await?;

    // 4.5 假期类型初始 7 类（业务域 hr/time-off 的基线数据）。幂等口径：按 `type_code`
    //     查重且**不加软删过滤**——`uk_hr_time_off_type_code` 是单列唯一键，软删行仍占位
    //     （同 `sys_position.position_code`）；命中即跳过，不覆盖运维改过的行。
    //     `status` = 1 启用（`time_off::TIME_OFF_TYPE_ENABLED`）；`pay_ratio` 取 DDL 默认
    //     1000（全额计薪）、`remark` 留空，故此处不 Set。
    const SEED_LEAVE_TYPES: &[(&str, &str, i8, i8, i32, i8, i8)] = &[
        // (type_code, type_name, unit, balance_mode, min_unit_minutes, need_attachment, allow_negative)
        ("annual", "年假", 1, 1, 240, 0, 0),
        ("comp", "调休", 2, 1, 60, 0, 0),
        ("personal", "事假", 1, 0, 240, 0, 0),
        ("sick", "病假", 1, 0, 240, 1, 0),
        ("marriage", "婚假", 1, 1, 480, 1, 0),
        ("maternity", "产假", 1, 1, 480, 1, 0),
        ("bereavement", "丧假", 1, 1, 480, 0, 0),
    ];
    for (code, name, unit, mode, min_minutes, need_attachment, allow_negative) in SEED_LEAVE_TYPES {
        let existing = hr_time_off_type::Entity::find()
            .filter(hr_time_off_type::Column::TypeCode.eq(*code))
            .one(db)
            .await?;
        if existing.is_none() {
            hr_time_off_type::ActiveModel {
                type_code: Set(String::from(*code)),
                type_name: Set(String::from(*name)),
                unit: Set(*unit),
                balance_mode: Set(*mode),
                min_unit_minutes: Set(*min_minutes),
                require_attachment: Set(*need_attachment),
                allow_negative: Set(*allow_negative),
                status: Set(1),
                // 种子数据的操作人统一记为 admin 自己
                created_by: Set(admin_id),
                updated_by: Set(admin_id),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }

    // 5. 示例定时任务：登录日志每日清理（幂等按 job_name；调度器在 init_scheduler 装载）
    const SEED_JOB_NAME: &str = "登录日志每日清理";
    let sample_job = sys_job::Entity::find()
        .filter(sys_job::Column::JobName.eq(SEED_JOB_NAME))
        .one(db)
        .await?;
    if sample_job.is_none() {
        sys_job::ActiveModel {
            job_name: Set(SEED_JOB_NAME.to_string()),
            cron_expr: Set("0 30 3 * * *".to_string()),
            handler_name: Set(crate::task::login_log_cleanup::HANDLER_NAME.to_string()),
            status: Set(1),
            remark: Set("种子示例：每日 03:30:00 清理 90 天前登录日志".to_string()),
            created_by: Set(admin_id),
            updated_by: Set(admin_id),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }

    // 5.1 刷新凭证每日清理（幂等按 job_name；调度器在 init_scheduler 装载）
    const SEED_SESSION_JOB_NAME: &str = "刷新凭证每日清理";
    let session_job = sys_job::Entity::find()
        .filter(sys_job::Column::JobName.eq(SEED_SESSION_JOB_NAME))
        .one(db)
        .await?;
    if session_job.is_none() {
        sys_job::ActiveModel {
            job_name: Set(SEED_SESSION_JOB_NAME.to_string()),
            cron_expr: Set("0 30 4 * * *".to_string()),
            handler_name: Set(crate::task::refresh_token_cleanup::HANDLER_NAME.to_string()),
            status: Set(1),
            remark: Set("每日 04:30:00 清理过期超 30 天的登录刷新凭证".to_string()),
            created_by: Set(admin_id),
            updated_by: Set(admin_id),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }

    // 5.2 假期额度过期作废（幂等按 job_name；调度器在 init_scheduler 装载，
    //     handler 由 task 注册表提供，见 task/mod.rs 的 handlers()）
    const SEED_LEAVE_EXPIRE_JOB_NAME: &str = "假期额度过期作废";
    let time_off_expire_job = sys_job::Entity::find()
        .filter(sys_job::Column::JobName.eq(SEED_LEAVE_EXPIRE_JOB_NAME))
        .one(db)
        .await?;
    if time_off_expire_job.is_none() {
        sys_job::ActiveModel {
            job_name: Set(SEED_LEAVE_EXPIRE_JOB_NAME.to_string()),
            cron_expr: Set("0 30 1 * * *".to_string()),
            handler_name: Set(crate::task::time_off_grant_expire::HANDLER_NAME.to_string()),
            status: Set(1),
            remark: Set("每日 01:30:00 作废已过失效期的假期额度批次".to_string()),
            created_by: Set(admin_id),
            updated_by: Set(admin_id),
            ..Default::default()
        }
        .insert(db)
        .await?;
    }

    // 6. super 角色绑定全部菜单：缺失的关联补上
    for menu_id in menu_ids_by_name.values() {
        let bound = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(super_role_id))
            .filter(sys_role_menu::Column::MenuId.eq(*menu_id))
            .one(db)
            .await?;
        if bound.is_none() {
            sys_role_menu::ActiveModel {
                role_id: Set(super_role_id),
                menu_id: Set(*menu_id),
            }
            .insert(db)
            .await?;
        }
    }

    // 7. API 权限点种子：按 path + method 查重（含软删行，唯一索引物理占位）。
    //    只补缺、不覆盖是刻意设计：管理员后续修改的 description / api_group /
    //    status（禁用即放行）不会被启动重置；命中软删行仅告警跳过（不复活），
    //    既尊重删除意图，也避开 uk_api_path_method 唯一索引冲突。
    for seed in API_SEEDS {
        // 登记侧规范化：与判定面 canonical_path 同一规则，防止种子里写含尾斜杠/
        // 重复斜杠/转义的非规范形式而永远匹配不上（那会让该接口静默退回 fail-open）。
        let path = match canonical_api_path(seed.path) {
            Ok(path) => path,
            Err(err) => {
                tracing::error!(%err, "API 种子路径非法，跳过登记");
                continue;
            }
        };
        debug_assert_eq!(path, seed.path, "API 种子路径应已写成规范化形式");
        let existing = sys_api::Entity::find()
            .filter(sys_api::Column::Path.eq(path.as_str()))
            .filter(sys_api::Column::Method.eq(seed.method))
            .one(db)
            .await?;
        match existing {
            Some(api) if api.deleted_at.is_some() => {
                tracing::warn!("API 种子跳过软删占位行：{} {}", seed.method, path);
            }
            Some(_) => {}
            None => {
                sys_api::ActiveModel {
                    path: Set(path.clone()),
                    method: Set(seed.method.to_string()),
                    description: Set(seed.description.to_string()),
                    api_group: Set(seed.api_group.to_string()),
                    status: Set(1),
                    // 种子数据的操作人统一记为 admin 自己
                    created_by: Set(admin_id),
                    updated_by: Set(admin_id),
                    ..Default::default()
                }
                .insert(db)
                .await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 种子完整性：admin 可登录、super 角色存在、默认菜单与绑定齐全。
    #[tokio::test]
    async fn ensure_seed_creates_admin_super_and_default_menus() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let admin = sys_user::Entity::find()
            .filter(sys_user::Column::Username.eq(SEED_ADMIN_USERNAME))
            .one(&db)
            .await
            .unwrap()
            .expect("admin 应存在");
        assert!(
            crypt::verify_password(SEED_ADMIN_PASSWORD, &admin.password),
            "admin 密码应可登录（admin123）"
        );

        let super_role = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq(SEED_SUPER_ROLE_KEY))
            .one(&db)
            .await
            .unwrap()
            .expect("super 角色应存在");
        assert!(
            sys_user_role::Entity::find()
                .filter(sys_user_role::Column::UserId.eq(admin.id))
                .filter(sys_user_role::Column::RoleId.eq(super_role.id))
                .one(&db)
                .await
                .unwrap()
                .is_some(),
            "admin-super 绑定应存在"
        );

        let menu_count = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.is_in(MENU_SEEDS.iter().map(|s| s.name.to_string())))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(menu_count as usize, MENU_SEEDS.len(), "全部默认菜单应存在");

        let role_menu_count = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(super_role.id))
            .count(&db)
            .await
            .unwrap();
        assert!(
            role_menu_count >= MENU_SEEDS.len() as u64,
            "super 应绑定全部默认菜单"
        );
    }

    /// 幂等：连续执行两次，默认菜单数量不变（不重复插入）。
    #[tokio::test]
    async fn ensure_seed_is_idempotent() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let count_before = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.is_in(MENU_SEEDS.iter().map(|s| s.name.to_string())))
            .count(&db)
            .await
            .unwrap();

        ensure_seed(&db).await.unwrap();

        let count_after = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.is_in(MENU_SEEDS.iter().map(|s| s.name.to_string())))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(count_before, count_after, "重复播种不应产生重复菜单");
    }

    /// 审计字段自引用：admin 的 created_by / updated_by 应记为自身 id
    /// （自增 id 非 1 的库上也成立，避免指向不存在的用户）。
    #[tokio::test]
    async fn ensure_seed_admin_audit_fields_reference_itself() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let admin = sys_user::Entity::find()
            .filter(sys_user::Column::Username.eq(SEED_ADMIN_USERNAME))
            .one(&db)
            .await
            .unwrap()
            .expect("admin 应存在");
        assert_eq!(admin.created_by, admin.id, "created_by 应为 admin 自身 id");
        assert_eq!(admin.updated_by, admin.id, "updated_by 应为 admin 自身 id");
    }

    /// 操作日志菜单页面与删除按钮权限码应随种子就绪。
    #[tokio::test]
    async fn ensure_seed_creates_operation_log_menu_and_button() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let menu = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("SystemOperationLog"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("操作日志菜单应存在");
        assert_eq!(menu.menu_type, 2, "操作日志应为页面菜单");
        assert_eq!(menu.title, "操作日志");
        assert_eq!(menu.path, "/system/operation-log");
        let system = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("System"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("System 目录应存在");
        assert_eq!(menu.parent_id, system.id, "操作日志应挂在 System 目录下");

        let button = sys_menu::Entity::find()
            .filter(sys_menu::Column::Permission.eq("system:operation-log:delete"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("操作日志删除按钮权限码应存在");
        assert_eq!(button.menu_type, 3, "按钮应为菜单类型 3");
        assert_eq!(button.parent_id, menu.id, "按钮应挂在操作日志菜单下");
    }

    /// 职位管理菜单页面与三个按钮权限码应随种子就绪。
    #[tokio::test]
    async fn ensure_seed_creates_position_menu_and_buttons() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let menu = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("SystemPosition"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("职位管理菜单应存在");
        assert_eq!(menu.menu_type, 2, "职位管理应为页面菜单");
        assert_eq!(menu.title, "职位管理");
        assert_eq!(menu.path, "/system/position");
        let system = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("System"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("System 目录应存在");
        assert_eq!(menu.parent_id, system.id, "职位管理应挂在 System 目录下");

        let expected = [
            "system:position:create",
            "system:position:update",
            "system:position:delete",
        ];
        for permission in expected {
            let button = sys_menu::Entity::find()
                .filter(sys_menu::Column::Permission.eq(permission))
                .filter(sys_menu::Column::DeletedAt.is_null())
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("权限码 {permission} 应存在"));
            assert_eq!(button.menu_type, 3, "{permission} 应为按钮类型 3");
            assert_eq!(
                button.parent_id, menu.id,
                "{permission} 应挂在职位管理菜单下"
            );
        }
    }

    /// 登录日志菜单页面与删除按钮权限码应随种子就绪。
    #[tokio::test]
    async fn ensure_seed_creates_login_log_menu_and_button() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let menu = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("SystemLoginLog"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("登录日志菜单应存在");
        assert_eq!(menu.menu_type, 2, "登录日志应为页面菜单");
        assert_eq!(menu.title, "登录日志");
        assert_eq!(menu.path, "/system/login-log");
        let system = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("System"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("System 目录应存在");
        assert_eq!(menu.parent_id, system.id, "登录日志应挂在 System 目录下");

        let button = sys_menu::Entity::find()
            .filter(sys_menu::Column::Permission.eq("system:login-log:delete"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("登录日志删除按钮权限码应存在");
        assert_eq!(button.menu_type, 3, "按钮应为菜单类型 3");
        assert_eq!(button.parent_id, menu.id, "按钮应挂在登录日志菜单下");
    }

    /// 会话管理菜单页面与强制下线 / 删除按钮权限码应随种子就绪（前端页面
    /// views/system/session 依赖这两个码控制按钮显隐）。
    #[tokio::test]
    async fn ensure_seed_creates_session_menu_and_buttons() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let menu = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("SystemSession"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("会话管理菜单应存在");
        assert_eq!(menu.menu_type, 2, "会话管理应为页面菜单");
        assert_eq!(menu.title, "会话管理");
        assert_eq!(menu.path, "/system/session");
        assert_eq!(
            menu.component, "#/views/system/session/index.vue",
            "component 必须能被 views glob 匹配，否则前端路由 404"
        );
        let system = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("System"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("System 目录应存在");
        assert_eq!(menu.parent_id, system.id, "会话管理应挂在 System 目录下");

        for (name, permission) in [
            ("SystemSessionForceLogout", "system:session:force-logout"),
            ("SystemSessionDelete", "system:session:delete"),
        ] {
            let button = sys_menu::Entity::find()
                .filter(sys_menu::Column::Permission.eq(permission))
                .filter(sys_menu::Column::DeletedAt.is_null())
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{name} 按钮权限码应存在"));
            assert_eq!(button.menu_type, 3, "{name} 应为菜单类型 3");
            assert_eq!(button.parent_id, menu.id, "{name} 应挂在会话管理菜单下");
        }
    }

    /// 数据字典菜单页面与类型 / 字典项各三个按钮权限码应随种子就绪。
    #[tokio::test]
    async fn ensure_seed_creates_dictionary_menu_and_buttons() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let menu = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("SystemDictionary"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("数据字典菜单应存在");
        assert_eq!(menu.menu_type, 2, "数据字典应为页面菜单");
        assert_eq!(menu.title, "数据字典");
        assert_eq!(menu.path, "/system/dictionary");
        let system = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("System"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("System 目录应存在");
        assert_eq!(menu.parent_id, system.id, "数据字典应挂在 System 目录下");

        let expected = [
            "system:dictionary:create",
            "system:dictionary:update",
            "system:dictionary:delete",
            "system:dictionary-detail:create",
            "system:dictionary-detail:update",
            "system:dictionary-detail:delete",
        ];
        for permission in expected {
            let button = sys_menu::Entity::find()
                .filter(sys_menu::Column::Permission.eq(permission))
                .filter(sys_menu::Column::DeletedAt.is_null())
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("权限码 {permission} 应存在"));
            assert_eq!(button.menu_type, 3, "{permission} 应为按钮类型 3");
            assert_eq!(
                button.parent_id, menu.id,
                "{permission} 应挂在数据字典菜单下"
            );
        }
    }

    /// 取指定字典项 extend（测试辅助：按 value 精确取字典项）。
    async fn dict_item_extend(db: &DatabaseConnection, dict_id: u64, value: &str) -> String {
        sys_dictionary_detail::Entity::find()
            .filter(sys_dictionary_detail::Column::DictionaryId.eq(dict_id))
            .filter(sys_dictionary_detail::Column::Value.eq(value))
            .one(db)
            .await
            .unwrap()
            .expect("字典项应存在")
            .extend
    }

    /// 覆盖指定字典项 extend（测试辅助：模拟脏数据 / 运维自定义）。
    async fn set_dict_item_extend(
        db: &DatabaseConnection,
        dict_id: u64,
        value: &str,
        extend: &str,
    ) {
        let item = sys_dictionary_detail::Entity::find()
            .filter(sys_dictionary_detail::Column::DictionaryId.eq(dict_id))
            .filter(sys_dictionary_detail::Column::Value.eq(value))
            .one(db)
            .await
            .unwrap()
            .expect("字典项应存在");
        let mut active: sys_dictionary_detail::ActiveModel = item.into();
        active.extend = Set(extend.to_string());
        active.update(db).await.unwrap();
    }

    /// execResultStatus 字典项 extend 契约（前端 Tag 颜色）：成功=success、失败=danger。
    /// seed 幂等补写：历史空串 extend 被回填，运维自定义非空值不被覆盖。
    #[tokio::test]
    async fn ensure_seed_writes_and_backfills_exec_result_status_extend() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let dict_id = sys_dictionary::Entity::find()
            .filter(sys_dictionary::Column::Type.eq("execResultStatus"))
            .one(&db)
            .await
            .unwrap()
            .expect("execResultStatus 字典应存在")
            .id;

        // 1. 种子契约：成功 → success、失败 → danger
        assert_eq!(dict_item_extend(&db, dict_id, "1").await, "success");
        assert_eq!(dict_item_extend(&db, dict_id, "0").await, "danger");

        // 2. 历史脏数据（extend 空串）：ensure_seed 应回填默认颜色
        set_dict_item_extend(&db, dict_id, "1", "").await;
        ensure_seed(&db).await.unwrap();
        assert_eq!(
            dict_item_extend(&db, dict_id, "1").await,
            "success",
            "空串 extend 应被 seed 回填"
        );

        // 3. 运维自定义非空 extend：ensure_seed 不覆盖
        set_dict_item_extend(&db, dict_id, "1", "warning").await;
        ensure_seed(&db).await.unwrap();
        assert_eq!(
            dict_item_extend(&db, dict_id, "1").await,
            "warning",
            "非空 extend 应保留运维自定义"
        );

        // 收尾恢复默认，保持库状态收敛（中途 panic 遗留非空值也可手动复位）
        set_dict_item_extend(&db, dict_id, "1", "success").await;
    }

    /// 人事枚举字典：`employmentStatus` / `education` 的启用项即员工档案
    /// `employment_status` / `education` 的合法取值集合（接口层用同一函数预取）。
    /// 字典缺失会导致建档 / 改档全部被拒（`get_dictionary_by_type` 报错），
    /// 故值集合必须与 `hr_employee` 列注释一致。
    #[tokio::test]
    async fn ensure_seed_creates_hr_employee_dictionaries() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let mut status_allowed = crate::modules::system::dictionary::service::enabled_int_values(
            &db,
            "employmentStatus",
        )
        .await
        .unwrap();
        status_allowed.sort_unstable();
        assert_eq!(status_allowed, vec![1, 2, 3], "在职状态：在职/试用/离职");

        let mut education_allowed =
            crate::modules::system::dictionary::service::enabled_int_values(&db, "education")
                .await
                .unwrap();
        education_allowed.sort_unstable();
        assert_eq!(
            education_allowed,
            vec![0, 1, 2, 3, 4, 5],
            "学历：未填/高中/大专/本科/硕士/博士"
        );
    }

    /// 员工档案端点必须全部登记：漏登 = 该端点 fail-open（未登记即放行），
    /// 所以断言的是「5 条都在、都是 POST、都启用且未软删」，而不只是菜单存在。
    #[tokio::test]
    async fn ensure_seed_registers_hr_employee_endpoints_and_menus() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        for action in ["list", "create", "update", "get", "delete"] {
            let path = format!("/api/v1/hr/employee/{action}");
            let row = sys_api::Entity::find()
                .filter(sys_api::Column::Path.eq(path.as_str()))
                .filter(sys_api::Column::Method.eq("POST"))
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{path} 必须登记在 sys_api"));
            assert_eq!(row.status, 1, "{path} 种子应为启用");
            assert!(row.deleted_at.is_none(), "{path} 不应是软删占位行");
        }

        let hr_id = find_menu_id_by_name(&db, "Hr")
            .await
            .unwrap()
            .expect("Hr 目录");
        let employee_id = find_menu_id_by_name(&db, "HrEmployee")
            .await
            .unwrap()
            .expect("HrEmployee 菜单");
        let employee = sys_menu::Entity::find_by_id(employee_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(employee.parent_id, hr_id, "员工档案页应挂在人事管理目录下");
        assert_eq!(employee.menu_type, 2, "员工档案应是页面菜单");
        assert_eq!(employee.component, "#/views/biz/hr/employee/index.vue");

        for (name, permission) in [
            ("HrEmployeeCreate", "hr:employee:create"),
            ("HrEmployeeUpdate", "hr:employee:update"),
            ("HrEmployeeDelete", "hr:employee:delete"),
        ] {
            let id = find_menu_id_by_name(&db, name).await.unwrap().expect(name);
            let button = sys_menu::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(button.parent_id, employee_id, "{name} 应挂在页面菜单下");
            assert_eq!(button.menu_type, 3, "{name} 应是按钮");
            assert_eq!(button.permission, permission);
        }
    }

    /// API 种子：全部管理端点应随 ensure_seed 落库（含可能被管理员软删的
    /// 占位行——库中曾有同 path+method 软删行时种子按设计跳过，不复活）。
    #[tokio::test]
    async fn ensure_seed_creates_all_api_seeds() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        // 不过滤 deleted_at：每条种子在库中应有且仅有一行（唯一索引保证）
        let count = sys_api::Entity::find()
            .filter(sys_api::Column::Path.is_in(API_SEEDS.iter().map(|s| s.path.to_string())))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(count as usize, API_SEEDS.len(), "全部 API 种子应存在");

        // 契约端点不应被登记（登记即 fail-closed 会拒绝非超管用户）
        for contract_path in [
            "/api/v1/user/info",
            "/api/v1/user/access-codes",
            "/api/v1/user/menus",
        ] {
            let leaked = sys_api::Entity::find()
                .filter(sys_api::Column::Path.eq(contract_path))
                .count(&db)
                .await
                .unwrap();
            assert_eq!(leaked, 0, "{contract_path} 不应登记");
        }
    }

    /// API 种子路径必须是规范化写法：判定面查 `sys_api` 前会把请求路径规范化
    /// （逐段 decode + 丢弃空段 + 去尾斜杠），登记成含尾斜杠/重复斜杠/转义的
    /// 形式会永远匹配不上，该接口会静默退回 fail-open（2026-09-17 修）。
    #[test]
    fn api_seeds_use_canonical_paths() {
        for seed in API_SEEDS {
            assert!(
                !seed.path.contains('%'),
                "种子路径不得含转义：{} {}",
                seed.method,
                seed.path
            );
            assert!(
                !seed.path.contains("//"),
                "种子路径不得含重复斜杠：{} {}",
                seed.method,
                seed.path
            );
            assert!(
                !seed.path.ends_with('/'),
                "种子路径不得带尾斜杠：{} {}",
                seed.method,
                seed.path
            );
            assert!(
                seed.path.starts_with("/api/v1/"),
                "种子路径应形如 /api/v1/...：{} {}",
                seed.method,
                seed.path
            );
        }
    }

    /// API 种子幂等：连续执行两次，条数不变（不重复插入、不覆盖）。
    #[tokio::test]
    async fn ensure_seed_api_is_idempotent() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let count_before = sys_api::Entity::find()
            .filter(sys_api::Column::Path.is_in(API_SEEDS.iter().map(|s| s.path.to_string())))
            .count(&db)
            .await
            .unwrap();

        ensure_seed(&db).await.unwrap();

        let count_after = sys_api::Entity::find()
            .filter(sys_api::Column::Path.is_in(API_SEEDS.iter().map(|s| s.path.to_string())))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(count_before, count_after, "重复播种不应产生重复 API 记录");
    }

    /// API 种子遇软删占位行：跳过不复活、不撞唯一索引、不插入新行。
    #[tokio::test]
    async fn ensure_seed_api_skips_soft_deleted_row() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        // 软删一条种子记录（path+method 物理占位）
        let target = sys_api::Entity::find()
            .filter(sys_api::Column::Path.eq("/api/v1/role/list"))
            .filter(sys_api::Column::Method.eq("POST"))
            .filter(sys_api::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("种子行应存在");
        let mut soft_deleted: sys_api::ActiveModel = target.into();
        soft_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
        soft_deleted.update(&db).await.unwrap();

        ensure_seed(&db).await.unwrap();

        let still_deleted = sys_api::Entity::find()
            .filter(sys_api::Column::Path.eq("/api/v1/role/list"))
            .filter(sys_api::Column::Method.eq("POST"))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(still_deleted.len(), 1, "不应插入新行（唯一索引占位）");
        assert!(still_deleted[0].deleted_at.is_some(), "软删行不应被复活");

        // 还原为启用行，避免污染其他测试 / 本地库
        let mut restored: sys_api::ActiveModel = still_deleted[0].clone().into();
        restored.deleted_at = Set(None);
        restored.update(&db).await.unwrap();
    }

    /// `sys_menu.name` 唯一索引兜底（并发 seed 防重）：同名第二行应被数据库拒绝。
    #[tokio::test]
    async fn menu_name_unique_index_rejects_duplicate_names() {
        let db = test_db().await;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!("UniqueMenuTest_{unique}");

        let first = sys_menu::ActiveModel {
            parent_id: Set(0),
            title: Set("唯一索引测试".to_string()),
            name: Set(name.clone()),
            menu_type: Set(2),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await;
        assert!(first.is_ok(), "首个菜单应插入成功: {first:?}");
        let first_id = first.unwrap().id;

        let second = sys_menu::ActiveModel {
            parent_id: Set(0),
            title: Set("唯一索引测试重复".to_string()),
            name: Set(name),
            menu_type: Set(2),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await;
        assert!(second.is_err(), "同名菜单应被唯一索引拒绝: {second:?}");

        // 直连真库、非事务：手工清理
        sys_menu::Entity::delete_by_id(first_id)
            .exec(&db)
            .await
            .unwrap();
    }

    /// 假期额度域种子（业务域 `hr/time-off`）：菜单父子链与组件路径、12 条端点登记、
    /// 发放依据字典、初始 7 类假期类型、过期作废任务行——连跑两次后每项都必须
    /// 恰好一份（幂等）。端点漏登 = 该端点 fail-open（未登记放行），所以断言的是
    /// 「12 条都在、都是 POST、都启用且未软删」，而不只是菜单存在。
    #[tokio::test]
    async fn ensure_seed_time_off_foundation_is_idempotent() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();
        ensure_seed(&db).await.unwrap();

        for path in [
            "/api/v1/hr/time-off/type/list",
            "/api/v1/hr/time-off/type/create",
            "/api/v1/hr/time-off/type/update",
            "/api/v1/hr/time-off/type/get",
            "/api/v1/hr/time-off/type/delete",
            "/api/v1/hr/time-off/grant/list",
            "/api/v1/hr/time-off/grant/batch-create",
            "/api/v1/hr/time-off/grant/get",
            "/api/v1/hr/time-off/grant/cancel",
            "/api/v1/hr/time-off/balance/list",
            "/api/v1/hr/time-off/balance/get",
            "/api/v1/hr/time-off/balance/logs",
        ] {
            let row = sys_api::Entity::find()
                .filter(sys_api::Column::Path.eq(path))
                .filter(sys_api::Column::Method.eq("POST"))
                .one(&db)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{path} 必须登记在 sys_api"));
            assert_eq!(row.status, 1, "{path} 种子应为启用");
            assert!(row.deleted_at.is_none(), "{path} 不应是软删占位行");
        }

        let hr_id = find_menu_id_by_name(&db, "Hr")
            .await
            .unwrap()
            .expect("Hr 目录");
        let time_off_id = find_menu_id_by_name(&db, "HrTimeOff")
            .await
            .unwrap()
            .expect("HrTimeOff 菜单");
        let time_off_menu = sys_menu::Entity::find_by_id(time_off_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            time_off_menu.parent_id, hr_id,
            "假期管理应挂在人事管理目录下"
        );
        assert_eq!(time_off_menu.menu_type, 1, "假期管理应是目录菜单");

        for (name, component) in [
            ("HrTimeOffType", "#/views/biz/hr/time-off/type/index.vue"),
            ("HrTimeOffGrant", "#/views/biz/hr/time-off/grant/index.vue"),
            (
                "HrTimeOffBalance",
                "#/views/biz/hr/time-off/balance/index.vue",
            ),
        ] {
            let id = find_menu_id_by_name(&db, name).await.unwrap().expect(name);
            let menu = sys_menu::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(menu.parent_id, time_off_id, "{name} 应挂在假期管理目录下");
            assert_eq!(menu.menu_type, 2, "{name} 应是页面菜单");
            assert_eq!(menu.component, component);
        }

        for (name, parent, permission) in [
            (
                "HrTimeOffTypeCreate",
                "HrTimeOffType",
                "hr:time-off-type:create",
            ),
            (
                "HrTimeOffTypeUpdate",
                "HrTimeOffType",
                "hr:time-off-type:update",
            ),
            (
                "HrTimeOffTypeDelete",
                "HrTimeOffType",
                "hr:time-off-type:delete",
            ),
            (
                "HrTimeOffGrantCreate",
                "HrTimeOffGrant",
                "hr:time-off-grant:create",
            ),
            (
                "HrTimeOffGrantCancel",
                "HrTimeOffGrant",
                "hr:time-off-grant:cancel",
            ),
        ] {
            let id = find_menu_id_by_name(&db, name).await.unwrap().expect(name);
            let button = sys_menu::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            let parent_id = find_menu_id_by_name(&db, parent)
                .await
                .unwrap()
                .expect(parent);
            assert_eq!(button.parent_id, parent_id, "{name} 应挂在 {parent} 下");
            assert_eq!(button.menu_type, 3, "{name} 应是按钮");
            assert_eq!(button.permission, permission);
        }

        // 发放依据字典：7 项，value 即 hr_time_off_grant.reason 的编码
        let reason_dict = sys_dictionary::Entity::find()
            .filter(sys_dictionary::Column::Type.eq("timeOffGrantReason"))
            .one(&db)
            .await
            .unwrap()
            .expect("timeOffGrantReason 字典应存在");
        let mut reasons = sys_dictionary_detail::Entity::find()
            .filter(sys_dictionary_detail::Column::DictionaryId.eq(reason_dict.id))
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.value)
            .collect::<Vec<_>>();
        reasons.sort();
        assert_eq!(
            reasons,
            vec![
                "carry",
                "comp",
                "company",
                "manual",
                "seniority",
                "severance",
                "statutory"
            ],
            "发放依据字典应恰好 7 项且不重复"
        );

        // 初始 7 类假期类型：type_code 各 1 行、启用、值等于基线表
        for (code, type_name, unit, mode, min_minutes, need_attachment, allow_negative) in [
            ("annual", "年假", 1, 1, 240, 0, 0),
            ("comp", "调休", 2, 1, 60, 0, 0),
            ("personal", "事假", 1, 0, 240, 0, 0),
            ("sick", "病假", 1, 0, 240, 1, 0),
            ("marriage", "婚假", 1, 1, 480, 1, 0),
            ("maternity", "产假", 1, 1, 480, 1, 0),
            ("bereavement", "丧假", 1, 1, 480, 0, 0),
        ] {
            let rows = hr_time_off_type::Entity::find()
                .filter(hr_time_off_type::Column::TypeCode.eq(code))
                .all(&db)
                .await
                .unwrap();
            assert_eq!(rows.len(), 1, "假期类型 {code} 应恰好 1 行（幂等）");

            let row = &rows[0];
            assert_eq!(row.type_name, type_name);
            assert_eq!(row.unit, unit);
            assert_eq!(row.balance_mode, mode);
            assert_eq!(row.min_unit_minutes, min_minutes);
            assert_eq!(row.require_attachment, need_attachment);
            assert_eq!(row.allow_negative, allow_negative);
            assert_eq!(row.status, 1, "假期类型 {code} 应为启用");
            assert!(row.deleted_at.is_none(), "种子假期类型 {code} 不应软删");
        }

        // 过期作废任务：1 行且 handler 名与 task 注册表一致
        let jobs = sys_job::Entity::find()
            .filter(sys_job::Column::JobName.eq("假期额度过期作废"))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(jobs.len(), 1, "过期作废任务应恰好 1 行（幂等）");
        assert_eq!(
            jobs[0].handler_name,
            crate::task::time_off_grant_expire::HANDLER_NAME
        );
    }
}
