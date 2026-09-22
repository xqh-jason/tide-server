//! HR 假期额度域：假期类型 + 额度账本（授予批次 / 聚合账户 / append-only 流水）+ 请假单。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

// —— 假期额度域常量唯一定义点（复制到别处即漂移）——
pub const TIME_OFF_TYPE_ENABLED: i8 = 1;
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
pub const LOG_BIZ_TIME_OFF_LOCK: i8 = 3;
pub const LOG_BIZ_TIME_OFF_CONSUME: i8 = 4;
pub const LOG_BIZ_TIME_OFF_RELEASE: i8 = 5;
pub const LOG_BIZ_EXPIRE: i8 = 6;

/// 来源对象类型（`hr_time_off_grant.source_kind` 与 `hr_time_off_balance_log.source_kind`
/// **同口径**，取值见 DDL 注释）：
/// - `0`：无来源（HR 发放 / 手工调整，幂等键用 `reason + period`）；
/// - `1`：系统任务（定时任务写入）；
/// - `2`：请假单（预占 / 实扣 / 释放 / 归还批次的来源单据）；
/// - `3`：加班单（加班转调休批次的来源单据）；
/// - `4`：手工操作（HR 在页面上发放 / 调整）。
///
/// `source_kind != 0` 时批次幂等键改用 `(employee, type, source_kind, source_id)`——
/// 否则「同一年第二次加班转调休」会被四列旧键吞掉。
pub const SOURCE_KIND_NONE: i8 = 0;
pub const SOURCE_KIND_JOB: i8 = 1;
pub const SOURCE_KIND_TIME_OFF: i8 = 2;
pub const SOURCE_KIND_OVERTIME: i8 = 3;
pub const SOURCE_KIND_MANUAL: i8 = 4;

/// 调休假别编码（种子 `hr_time_off_type.type_code`）：加班转调休的入账落点。
pub const TIME_OFF_TYPE_CODE_COMPENSATORY: &str = "comp";
/// 发放依据：加班转调休（字典 `timeOffGrantReason` 的既有项）。
pub const GRANT_REASON_OVERTIME: &str = "comp";
/// 发放依据：驳回释放归还（字典 `timeOffGrantReason` 的项；归还到当前账期的批次用）。
pub const GRANT_REASON_RELEASE_RESTORE: &str = "releaseRestore";

/// 请假单状态：审批中。
pub const REQUEST_STATUS_PENDING: i8 = 1;
/// 请假单状态：已通过。
pub const REQUEST_STATUS_APPROVED: i8 = 2;
/// 请假单状态：已驳回。
pub const REQUEST_STATUS_REJECTED: i8 = 3;
/// 请假单状态：已撤销。
pub const REQUEST_STATUS_CANCELED: i8 = 4;

/// 假期额度端点：`POST /api/v1/hr/time-off/{type,grant,balance,request}/{...}`。
///
/// 路由在此注册，由 `modules/mod.rs` 的 `DOMAINS` 登记表挂到 `hr/time-off` 前缀下
/// （`MountGuard::Protected` → 自动获得 AuthRequired + OperationLog + ApiPermission 三件套）；
/// 每个端点都必须登记进 `sys_api`，漏登 = 该端点 fail-open（见 seed.rs 的 API_SEEDS）。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["假期额度"])
        .push(
            Router::with_path("type")
                .push(Router::with_path("list").post(api::list_time_off_types))
                .push(Router::with_path("create").post(api::create_time_off_type))
                .push(Router::with_path("update").post(api::update_time_off_type))
                .push(Router::with_path("get").post(api::get_time_off_type))
                .push(Router::with_path("delete").post(api::delete_time_off_type)),
        )
        .push(
            Router::with_path("grant")
                .push(Router::with_path("list").post(api::list_time_off_grants))
                .push(Router::with_path("batch-create").post(api::batch_create_time_off_grants))
                .push(Router::with_path("get").post(api::get_time_off_grant))
                .push(Router::with_path("cancel").post(api::cancel_time_off_grant)),
        )
        .push(
            Router::with_path("balance")
                .push(Router::with_path("list").post(api::list_time_off_balances))
                .push(Router::with_path("get").post(api::get_time_off_balance))
                .push(Router::with_path("logs").post(api::list_time_off_balance_logs)),
        )
        .push(
            Router::with_path("request")
                .push(Router::with_path("list").post(api::list_time_off_requests))
                .push(Router::with_path("create").post(api::create_time_off_request))
                .push(Router::with_path("update").post(api::update_time_off_request))
                .push(Router::with_path("get").post(api::get_time_off_request))
                .push(Router::with_path("delete").post(api::delete_time_off_request))
                .push(Router::with_path("submit").post(api::submit_time_off_request))
                .push(Router::with_path("cancel").post(api::cancel_time_off_request))
                .push(Router::with_path("mine").post(api::list_my_time_off_requests)),
        )
}
