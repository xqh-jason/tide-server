//! HR 考勤域：班次 / 排班 / 出勤事实 / 工作日历（公司排班制考勤）。
//!
//! 四张表的分工与写入口径（DDL 事实来源见
//! `migrations/src/m20260922_000004_create_hr_attendance.rs`）：
//! - `hr_shift`：班次定义，**唯一软删主表**（会被历史排班引用，删了老排班仍要能取名）；
//! - `hr_shift_schedule` / `hr_attendance_record` / `hr_work_calendar`：排班 / 事实 / 日历，
//!   **不软删**，写入一律走「按唯一键 upsert」（软删占位会让 `(employee_id, work_date)` 与
//!   `calendar_date` 唯一键在重录时撞键）；
//! - 「应出勤」不落冗余列，由「排班 × 日历」在 service 层实时派生：
//!   [`service::resolve_workday`]（某员工某日的应出勤窗口）与
//!   [`service::derive_work_minutes`]（区间内应出勤分钟数）是 P2 请假 / P4 加班的跨域冻结契约；
//! - 迟到 / 早退 / 缺卡只做**事实展示**（P5 薪酬不做，不产生扣减）。
//!
//! 对外端点：`POST /api/v1/hr/attendance/{shift,schedule,record,calendar}/{...}`，共 16 个。
//! 路由在此注册，由 `modules/mod.rs` 的 `DOMAINS` 登记表挂到 `hr/attendance` 前缀下
//! （`MountGuard::Protected` → 自动获得 AuthRequired + OperationLog + ApiPermission 三件套）；
//! 每个端点都必须登记进 `sys_api`，漏登 = 该端点 fail-open（见 seed.rs 的 API_SEEDS）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

// —— 考勤域常量唯一定义点（复制到别处即漂移）——

/// 班次状态：1 启用。
pub const SHIFT_STATUS_ENABLED: i8 = 1;
/// 班次状态：0 停用。
pub const SHIFT_STATUS_DISABLED: i8 = 0;

/// 排班状态：1 正常。
pub const SCHEDULE_STATUS_NORMAL: i8 = 1;
/// 排班状态：2 已换班。
pub const SCHEDULE_STATUS_SWAPPED: i8 = 2;
/// 排班网格占位状态：0 未排班（仅 `schedule/month` 网格响应使用，**不落库**）。
pub const SCHEDULE_STATUS_UNSCHEDULED: i8 = 0;
/// 排班状态全部取值（validate 用）。
pub const SCHEDULE_STATUSES: [i8; 2] = [SCHEDULE_STATUS_NORMAL, SCHEDULE_STATUS_SWAPPED];

/// 出勤来源：1 导入。
pub const SOURCE_IMPORT: i8 = 1;
/// 出勤来源：2 手工补录。
pub const SOURCE_MANUAL: i8 = 2;
/// 出勤来源：3 设备。
pub const SOURCE_DEVICE: i8 = 3;
/// 出勤来源：4 钉钉。
pub const SOURCE_DINGTALK: i8 = 4;
/// 出勤来源：5 飞书。
pub const SOURCE_FEISHU: i8 = 5;
/// 出勤来源全部取值（validate 用）。
pub const SOURCES: [i8; 5] = [
    SOURCE_IMPORT,
    SOURCE_MANUAL,
    SOURCE_DEVICE,
    SOURCE_DINGTALK,
    SOURCE_FEISHU,
];

/// 缺卡：0 无。
pub const MISS_CLOCK_NONE: i8 = 0;
/// 缺卡：1 缺上班卡。
pub const MISS_CLOCK_IN: i8 = 1;
/// 缺卡：2 缺下班卡。
pub const MISS_CLOCK_OUT: i8 = 2;
/// 缺卡：3 上下班卡都缺。
pub const MISS_CLOCK_BOTH: i8 = 3;

/// 日历类型：0 普通。
pub const HOLIDAY_TYPE_NORMAL: i8 = 0;
/// 日历类型：1 法定节假日。
pub const HOLIDAY_TYPE_STATUTORY: i8 = 1;
/// 日历类型：2 调休上班。
pub const HOLIDAY_TYPE_ADJUSTED_WORKDAY: i8 = 2;
/// 日历类型全部取值（validate 用）。
pub const HOLIDAY_TYPES: [i8; 3] = [
    HOLIDAY_TYPE_NORMAL,
    HOLIDAY_TYPE_STATUTORY,
    HOLIDAY_TYPE_ADJUSTED_WORKDAY,
];

/// 「排班与日历都查不到」时的工作日默认标准工时（分钟）——未配排班也不至于算 0。
pub const DEFAULT_STANDARD_MINUTES: i32 = 480;

/// 考勤域端点：`POST /api/v1/hr/attendance/{shift,schedule,record,calendar}/{...}`。
///
/// 函数顺序 = `api.rs` 函数顺序 = 本函数挂载顺序：
/// `shift` list → create → update → get → delete，`schedule` list → batch-create → update → month，
/// `record` list → get → update → import，`calendar` list → upsert → batch-import。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["考勤管理"])
        .push(
            Router::with_path("shift")
                .push(Router::with_path("list").post(api::list_shifts))
                .push(Router::with_path("create").post(api::create_shift))
                .push(Router::with_path("update").post(api::update_shift))
                .push(Router::with_path("get").post(api::get_shift))
                .push(Router::with_path("delete").post(api::delete_shift)),
        )
        .push(
            Router::with_path("schedule")
                .push(Router::with_path("list").post(api::list_schedules))
                .push(Router::with_path("batch-create").post(api::batch_create_schedules))
                .push(Router::with_path("update").post(api::update_schedule))
                .push(Router::with_path("month").post(api::month_schedules)),
        )
        .push(
            Router::with_path("record")
                .push(Router::with_path("list").post(api::list_records))
                .push(Router::with_path("get").post(api::get_record))
                .push(Router::with_path("update").post(api::update_record))
                .push(Router::with_path("import").post(api::import_records)),
        )
        .push(
            Router::with_path("calendar")
                .push(Router::with_path("list").post(api::list_calendars))
                .push(Router::with_path("upsert").post(api::upsert_calendar))
                .push(Router::with_path("batch-import").post(api::batch_import_calendars)),
        )
}
