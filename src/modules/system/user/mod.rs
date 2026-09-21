//! 用户域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 router.rs 挂到 /api/v1 下。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

/// 用户端点：`POST /api/v1/user/{list,by-username,info,access-codes,create,update,get,
/// get-depts,get-positions,update-status,delete,list-all,list-all-includes-soft-deleted}`。
/// 菜单契约端点 `menus` 在 menu 域的 `user_routes()` 中注册。
///
/// 曾并存过一条指向同一 handler 的重复路由 `get-depts-by-user-id`（与 `get-depts` 同
/// handler 不同 path）：`get-depts` 已登记 `sys_api` 而它未登记，于是该路径落在
/// fail-open 上被放行（任意登录用户可查任意用户的部门归属）。前端 0 处引用，
/// 已删除，使 `get-depts` 成为该 handler 的唯一入口。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["用户"])
        .push(Router::with_path("list").post(api::list_users))
        .push(Router::with_path("by-username").post(api::get_by_username))
        .push(Router::with_path("info").post(api::info))
        .push(Router::with_path("access-codes").post(api::access_codes))
        .push(Router::with_path("create").post(api::create_user))
        .push(Router::with_path("update").post(api::update_user))
        .push(Router::with_path("get").post(api::get_user))
        .push(Router::with_path("update-status").post(api::update_user_status))
        .push(Router::with_path("delete").post(api::delete_user))
        // 某用户挂载的部门列表（含部门名）：用户详情 / 表单回显用
        .push(Router::with_path("get-depts").post(api::get_depts_by_user_id))
        // 某用户挂载的职位列表（含职位名）：与 get-depts 对称的表单回显端点
        .push(Router::with_path("get-positions").post(api::get_positions_by_user_id))
        // 全量用户（含软删）：审计过滤的用户选择器数据源
        .push(
            Router::with_path("list-all-includes-soft-deleted")
                .post(api::list_all_users_includes_soft_deleted),
        )
        .push(Router::with_path("list-all").post(api::list_all_users))
}
