//! HR 加班域：加班单（申请 → 审批 → 补偿入账）。
//!
//! 一张表：`hr_overtime_request`（单据，软删主表）。
//!
//! 与请假域的三处差别（设计 §4.2）：
//! - **时长不裁剪**：加班本就发生在班次窗口之外，`duration_minutes` 直接取区间总长
//!   （分钟），派生规则只做「区间不得落在应工作窗口内」的**校验**，不按班次窗口裁剪；
//! - **类型必须与应出勤日匹配**：`overtime_type = 1 工作日` 要求 `work_date` 是应出勤日，
//!   `2 休息日` / `3 法定节假日` 要求 `work_date` **不是**应出勤日。应出勤与否由考勤域
//!   `attendance::service::resolve_workday`（排班 × 工作日历）实时派生，本域不自己判日历；
//! - **同日区间不重叠**：同一员工同一 `work_date` 的**在途（审批中）或已通过**加班单区间不得相交
//!   （`start_at < end_at_new AND end_at > start_at_new`）；只比对已通过的单据时，
//!   两张相交的在途单各自通过后会对同一时段重复补偿。
//!
//! 审批复用基座（`biz_type = "overtime"`）：建单即提交，同事务起审批实例并把
//! `approval_instance_id` 写回；终态由基座按 `biz_type` 分派回
//! [`service::on_instance_finished_in_tx`] ——
//! 通过 → 单据置「已通过」，`comp_mode = 1 转调休` 时在**同一事务内**调
//! `time_off::service::grant_time_off_in_tx` 生成调休批次（`source = 3`、
//! `source_kind = 3`、`source_id = 本单 ID`，靠来源幂等键防重复入账）；
//! `comp_mode = 2 计加班费` 只置状态（P5 薪酬不做金额）。
//!
//! 对外端点：`POST /api/v1/hr/overtime/{list,create,update,get,delete,submit,cancel,mine}`（8 个）。
//! 路由在此注册，由 `modules/mod.rs` 的 `DOMAINS` 登记表挂到 `hr/overtime` 前缀下
//! （`MountGuard::Protected` → 自动获得 AuthRequired + OperationLog + ApiPermission 三件套）；
//! 每个端点都必须登记进 `sys_api`，漏登 = 该端点 fail-open（见 seed.rs 的 API_SEEDS）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

// —— 加班域常量唯一定义点（复制到别处即漂移）——

/// 加班类型：工作日（`work_date` 必须是应出勤日）。
pub const OVERTIME_TYPE_WORKDAY: i8 = 1;
/// 加班类型：休息日（`work_date` 必须不是应出勤日）。
pub const OVERTIME_TYPE_REST_DAY: i8 = 2;
/// 加班类型：法定节假日（`work_date` 必须不是应出勤日）。
pub const OVERTIME_TYPE_HOLIDAY: i8 = 3;
/// 加班类型全部取值（`validate.rs` 的值域来源）。
pub const OVERTIME_TYPES: [i8; 3] = [
    OVERTIME_TYPE_WORKDAY,
    OVERTIME_TYPE_REST_DAY,
    OVERTIME_TYPE_HOLIDAY,
];

/// 补偿方式：转调休（审批通过后生成调休批次）。
pub const COMP_MODE_TIME_OFF: i8 = 1;
/// 补偿方式：计加班费（只落单据，P5 薪酬不做金额）。
pub const COMP_MODE_PAY: i8 = 2;
/// 补偿方式全部取值（`validate.rs` 的值域来源）。
pub const COMP_MODES: [i8; 2] = [COMP_MODE_TIME_OFF, COMP_MODE_PAY];

/// 单据状态：审批中。
pub const REQUEST_STATUS_PENDING: i8 = 1;
/// 单据状态：已通过。
pub const REQUEST_STATUS_APPROVED: i8 = 2;
/// 单据状态：已驳回。
pub const REQUEST_STATUS_REJECTED: i8 = 3;
/// 单据状态：已撤销（申请人撤回 / 单据删除）。
pub const REQUEST_STATUS_CANCELED: i8 = 4;
/// 单据状态全部取值（状态机取值集合的唯一定义处）。
///
/// 仅作域内声明：列表 / 我的单据的 `status` 过滤入参全仓一律透传、不做值域校验
/// （非法值自然返回空集），写入口的状态由状态机推进，不由请求体提供。
pub const REQUEST_STATUSES: [i8; 4] = [
    REQUEST_STATUS_PENDING,
    REQUEST_STATUS_APPROVED,
    REQUEST_STATUS_REJECTED,
    REQUEST_STATUS_CANCELED,
];

/// 调休批次有效期：加班转调休的调休自 `work_date` 起 **3 个自然月**内有效
/// （按月进位，月末不足取当月最后一天；不是 +90 天）。
pub const TIME_OFF_EXPIRE_MONTHS: u32 = 3;

/// 加班域端点：`POST /api/v1/hr/overtime/{list,create,update,get,delete,submit,cancel,mine}`。
///
/// 函数顺序 = `api.rs` 函数顺序 = 本函数挂载顺序：
/// list → create → update → get → delete → submit → cancel → mine。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["加班管理"])
        .push(Router::with_path("list").post(api::list_overtimes))
        .push(Router::with_path("create").post(api::create_overtime))
        .push(Router::with_path("update").post(api::update_overtime))
        .push(Router::with_path("get").post(api::get_overtime))
        .push(Router::with_path("delete").post(api::delete_overtime))
        .push(Router::with_path("submit").post(api::submit_overtime))
        .push(Router::with_path("cancel").post(api::cancel_overtime))
        .push(Router::with_path("mine").post(api::list_my_overtimes))
}
