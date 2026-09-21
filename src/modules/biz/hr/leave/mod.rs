//! HR 假期额度域：假期类型 + 额度账本（授予批次 / 聚合账户 / append-only 流水）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

// —— 假期额度域常量唯一定义点（复制到别处即漂移）——
pub const LEAVE_TYPE_ENABLED: i8 = 1;
pub const GRANT_STATUS_ACTIVE: i8 = 1;
pub const GRANT_STATUS_EXHAUSTED: i8 = 2;
pub const GRANT_STATUS_EXPIRED: i8 = 3;
pub const GRANT_STATUS_CANCELED: i8 = 4;
pub const GRANT_SOURCE_ISSUE: i8 = 1;
pub const GRANT_SOURCE_MANUAL: i8 = 2;
pub const GRANT_SOURCE_OVERTIME: i8 = 3;
pub const BALANCE_MODE_DEDUCT: i8 = 1;
pub const BALANCE_MODE_RECORD_ONLY: i8 = 0;
pub const LOG_BIZ_GRANT: i8 = 1;
pub const LOG_BIZ_ADJUST: i8 = 2;
pub const LOG_BIZ_LEAVE_LOCK: i8 = 3;
pub const LOG_BIZ_LEAVE_CONSUME: i8 = 4;
pub const LOG_BIZ_LEAVE_RELEASE: i8 = 5;
pub const LOG_BIZ_EXPIRE: i8 = 6;

/// 假期额度端点：`POST /api/v1/hr/leave/{leave-type,leave-grant,leave-balance}/{...}`。
///
/// 路由在此注册，由 `modules/mod.rs` 的 `DOMAINS` 登记表挂到 `hr/leave` 前缀下
/// （`MountGuard::Protected` → 自动获得 AuthRequired + OperationLog + ApiPermission 三件套）；
/// 每个端点都必须登记进 `sys_api`，漏登 = 该端点 fail-open（见 seed.rs 的 API_SEEDS）。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["假期额度"])
        .push(
            Router::with_path("leave-type")
                .push(Router::with_path("list").post(api::list_leave_types))
                .push(Router::with_path("create").post(api::create_leave_type))
                .push(Router::with_path("update").post(api::update_leave_type))
                .push(Router::with_path("get").post(api::get_leave_type))
                .push(Router::with_path("delete").post(api::delete_leave_type)),
        )
        .push(
            Router::with_path("leave-grant")
                .push(Router::with_path("list").post(api::list_leave_grants))
                .push(Router::with_path("batch-create").post(api::batch_create_leave_grants))
                .push(Router::with_path("get").post(api::get_leave_grant))
                .push(Router::with_path("cancel").post(api::cancel_leave_grant)),
        )
        .push(
            Router::with_path("leave-balance")
                .push(Router::with_path("list").post(api::list_leave_balances))
                .push(Router::with_path("get").post(api::get_leave_balance))
                .push(Router::with_path("logs").post(api::list_leave_balance_logs)),
        )
}
