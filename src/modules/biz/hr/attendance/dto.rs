//! 考勤域 DTO：列表请求 / repo 过滤条件 / 响应体声明。
//!
//! 约定（同 `hr/time_off`）：
//! - entity 不直接暴露给接口，响应体一律经 `From<Model>` 转换；
//! - 请求体永不接受 `*_by` / `*_name`（审计字段由 repo 盖章、人名字段由 service 拼装）；
//! - 时间字段一律 `String`（DTO 层不做解析）：日期 `yyyy-MM-dd`、到点时间 `HH:MM:SS`、
//!   打卡时间 `yyyy-MM-dd HH:mm:ss`；出参统一经 `utils::serde_format::format_datetime` /
//!   本文件的 `fmt_date` / `fmt_time` 格式化，入参由 `utils::datetime::parse_datetime` 解析；
//! - `employee_name` / `shift_code` / `shift_name` 由 service 批量拼装后回填；
//!   `created_by_name` / `updated_by_name` 由平台唯一管道 `utils::user_ref::fill_user_names`
//!   经 `UserRefNames` 填充（`From<Model>` 里一律留空串）；
//! - `hr_attendance_record.external_id` 是 `Option<String>`（本地补录为 `None`）。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{hr_attendance_record, hr_shift, hr_shift_schedule, hr_work_calendar};
use crate::utils::PageQuery;
use crate::utils::serde_format::format_datetime;
use crate::utils::user_ref::UserRefNames;

/// 日期格式化：`yyyy-MM-dd`（排班日 / 出勤日 / 日历日 / 月份）。
fn fmt_date(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// 到点时间格式化：`HH:MM:SS`（班次上下班时间）。
fn fmt_time(t: chrono::NaiveTime) -> String {
    t.format("%H:%M:%S").to_string()
}

/// 可选打卡时间格式化（`None` 保持 `None`）。
fn fmt_datetime_opt(t: Option<chrono::NaiveDateTime>) -> Option<String> {
    t.map(format_datetime)
}

/// `batch-create` / `update` 未指定排班状态时的缺省值：正常。
fn default_schedule_status() -> i8 {
    crate::modules::biz::hr::attendance::SCHEDULE_STATUS_NORMAL
}

// —— 班次 `hr_shift` ——

/// 班次列表请求：分页字段内嵌 `PageQuery`，过滤条件只在此声明一次。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShiftListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配班次编码 / 班次名称）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤（字典：1 启用 0 停用）；不传查全部
    pub status: Option<i8>,
}

/// 班次分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct ShiftFilter {
    /// 模糊搜索关键字（匹配班次编码 / 班次名称）
    pub keyword: Option<String>,
    /// 状态精确过滤（1 启用 0 停用）
    pub status: Option<i8>,
}

/// 创建班次请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateShiftReq {
    /// 班次编码（单列唯一，含软删占位）
    pub shift_code: String,
    /// 班次名称
    pub shift_name: String,
    /// 上班时间（班次开始，`HH:MM:SS`）
    pub start_time: String,
    /// 下班时间（班次结束，`HH:MM:SS`）
    pub end_time: String,
    /// 是否跨天班：1 是（`end_time` 落在次日）0 否
    pub cross_day: i8,
    /// 应工作分钟数（已扣除休息）
    pub work_minutes: i32,
    /// 休息分钟数
    pub rest_minutes: i32,
    /// 迟到宽限分钟数
    pub late_tolerance_minutes: i32,
    /// 是否需要打卡：1 需要 0 不需要
    pub need_clock: i8,
    /// 状态：1 启用 0 停用（值域来自平台字典 `status`）
    pub status: i8,
    /// 备注
    pub remark: String,
}

/// 更新班次请求（字段与创建一致 + 主键）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateShiftReq {
    /// 班次主键
    pub id: u64,
    /// 班次编码（单列唯一，含软删占位）
    pub shift_code: String,
    /// 班次名称
    pub shift_name: String,
    /// 上班时间（`HH:MM:SS`）
    pub start_time: String,
    /// 下班时间（`HH:MM:SS`）
    pub end_time: String,
    /// 是否跨天班：1 是 0 否
    pub cross_day: i8,
    /// 应工作分钟数（已扣除休息）
    pub work_minutes: i32,
    /// 休息分钟数
    pub rest_minutes: i32,
    /// 迟到宽限分钟数
    pub late_tolerance_minutes: i32,
    /// 是否需要打卡：1 需要 0 不需要
    pub need_clock: i8,
    /// 状态：1 启用 0 停用（值域来自平台字典 `status`）
    pub status: i8,
    /// 备注
    pub remark: String,
}

/// 班次响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShiftResp {
    pub id: u64,
    /// 班次编码
    pub shift_code: String,
    /// 班次名称
    pub shift_name: String,
    /// 上班时间（`HH:MM:SS`）
    pub start_time: String,
    /// 下班时间（`HH:MM:SS`）
    pub end_time: String,
    /// 是否跨天班：1 是 0 否
    pub cross_day: i8,
    /// 应工作分钟数（已扣除休息）
    pub work_minutes: i32,
    /// 休息分钟数
    pub rest_minutes: i32,
    /// 迟到宽限分钟数
    pub late_tolerance_minutes: i32,
    /// 是否需要打卡：1 需要 0 不需要
    pub need_clock: i8,
    /// 状态：1 启用 0 停用
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

/// `hr_shift::Model` → `ShiftResp` 字段搬运（人名字段留空待 `fill_user_names` 拼装）。
impl From<hr_shift::Model> for ShiftResp {
    fn from(m: hr_shift::Model) -> Self {
        Self {
            id: m.id,
            shift_code: m.shift_code,
            shift_name: m.shift_name,
            start_time: fmt_time(m.start_time),
            end_time: fmt_time(m.end_time),
            cross_day: m.cross_day,
            work_minutes: m.work_minutes,
            rest_minutes: m.rest_minutes,
            late_tolerance_minutes: m.late_tolerance_minutes,
            need_clock: m.need_clock,
            status: m.status,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for ShiftResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

// —— 排班 `hr_shift_schedule` ——

/// 排班列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 员工档案 ID 精确过滤；不传查全部
    pub employee_id: Option<u64>,
    /// 班次 ID 精确过滤（0 = 当天休息）；不传查全部
    pub shift_id: Option<u64>,
    /// 状态精确过滤（1 正常 2 已换班）；不传查全部
    pub status: Option<i8>,
    /// 排班日起（`yyyy-MM-dd`）；不传不设下限
    pub work_date_begin: Option<String>,
    /// 排班日止（`yyyy-MM-dd`）；不传不设上限
    pub work_date_end: Option<String>,
}

/// 排班分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct ScheduleFilter {
    /// 员工档案 ID 精确过滤
    pub employee_id: Option<u64>,
    /// 班次 ID 精确过滤（0 = 当天休息）
    pub shift_id: Option<u64>,
    /// 状态精确过滤（1 正常 2 已换班）
    pub status: Option<i8>,
    /// 排班日起（含）
    pub work_date_begin: Option<chrono::NaiveDate>,
    /// 排班日止（含）
    pub work_date_end: Option<chrono::NaiveDate>,
}

/// 批量排班请求：员工列表 × 日期区间，逐日 upsert（唯一键 `(employee_id, work_date)`）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateScheduleReq {
    /// 员工档案 ID 列表（去重后逐日排班）
    pub employee_ids: Vec<u64>,
    /// 区间起（`yyyy-MM-dd`，含）
    pub start_date: String,
    /// 区间止（`yyyy-MM-dd`，含）
    pub end_date: String,
    /// 班次 ID；0 = 排为休息
    pub shift_id: u64,
    /// 排班状态：1 正常 2 已换班；不传默认 1
    #[serde(default = "default_schedule_status")]
    pub status: i8,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 批量排班回执：命中已有排班即更新，否则新建。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateScheduleResp {
    /// 新建的排班行数
    pub created: u64,
    /// 更新（覆盖）的排班行数
    pub updated: u64,
}

/// 单日排班 upsert 请求（按 `(employeeId, workDate)` 定位，不存在即新建）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduleReq {
    /// 员工档案 ID
    pub employee_id: u64,
    /// 排班日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 班次 ID；0 = 当天休息
    pub shift_id: u64,
    /// 排班状态：1 正常 2 已换班
    pub status: i8,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 排班响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleResp {
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 排班日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 班次 ID；0 = 当天休息
    pub shift_id: u64,
    /// 班次编码（service 批量拼装；班次已软删时留空）
    pub shift_code: String,
    /// 班次名称（service 批量拼装；班次已软删时留空）
    pub shift_name: String,
    /// 状态：1 正常 2 已换班
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

/// `hr_shift_schedule::Model` → `ScheduleResp` 字段搬运（人名字段留空待拼装）。
impl From<hr_shift_schedule::Model> for ScheduleResp {
    fn from(m: hr_shift_schedule::Model) -> Self {
        Self {
            id: m.id,
            employee_id: m.employee_id,
            employee_name: String::new(),
            work_date: fmt_date(m.work_date),
            shift_id: m.shift_id,
            shift_code: String::new(),
            shift_name: String::new(),
            status: m.status,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for ScheduleResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 排班月视图请求：某月 × 某部门 / 全员。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleMonthReq {
    /// 月份（`yyyy-MM`）
    pub month: String,
    /// 部门 ID（该部门挂载账号的档案）；不传 = 全员（排除离职）
    pub dept_id: Option<u64>,
}

/// 排班月视图响应：一行 = 一个员工，`days` 覆盖该月每一天。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleMonthResp {
    /// 月份（`yyyy-MM`）
    pub month: String,
    /// 员工排班行
    pub employees: Vec<ScheduleMonthEmployeeResp>,
}

/// 月视图中的单个员工。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleMonthEmployeeResp {
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 该月每天的排班（按日期升序）
    pub days: Vec<ScheduleMonthDayResp>,
}

/// 月视图中的一天：无排班行时 `status = 0`（未排班占位，不落库）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleMonthDayResp {
    /// 日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 班次 ID；0 = 当天休息或未排班
    pub shift_id: u64,
    /// 班次编码（班次已软删时留空）
    pub shift_code: String,
    /// 班次名称（班次已软删时留空）
    pub shift_name: String,
    /// 状态：0 未排班 1 正常 2 已换班
    pub status: i8,
    /// 备注
    pub remark: String,
}

// —— 出勤事实 `hr_attendance_record` ——

/// 出勤事实列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 员工档案 ID 精确过滤；不传查全部
    pub employee_id: Option<u64>,
    /// 数据来源精确过滤（1 导入 2 手工补录 3 设备 4 钉钉 5 飞书）；不传查全部
    pub source: Option<i8>,
    /// 缺卡情况精确过滤（0 无 1 缺上班卡 2 缺下班卡 3 都缺）；不传查全部
    pub miss_clock: Option<i8>,
    /// 出勤日起（`yyyy-MM-dd`）；不传不设下限
    pub work_date_begin: Option<String>,
    /// 出勤日止（`yyyy-MM-dd`）；不传不设上限
    pub work_date_end: Option<String>,
}

/// 出勤事实分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct RecordFilter {
    /// 员工档案 ID 精确过滤
    pub employee_id: Option<u64>,
    /// 数据来源精确过滤
    pub source: Option<i8>,
    /// 缺卡情况精确过滤
    pub miss_clock: Option<i8>,
    /// 出勤日起（含）
    pub work_date_begin: Option<chrono::NaiveDate>,
    /// 出勤日止（含）
    pub work_date_end: Option<chrono::NaiveDate>,
}

/// 手工补录 / 修正出勤事实请求。
///
/// `clock_in` / `clock_out` 为 `None` = 保持原值，空串 = 清空；服务端按「打卡时间 vs 当日班次窗口」
/// 重算迟到 / 早退 / 实际出勤 / 缺卡，并刷新班次快照。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecordReq {
    /// 出勤事实主键
    pub id: u64,
    /// 上班打卡时间（`yyyy-MM-dd HH:mm:ss`；空串 = 清空，不传 = 不改）
    pub clock_in: Option<String>,
    /// 下班打卡时间（`yyyy-MM-dd HH:mm:ss`；空串 = 清空，不传 = 不改）
    pub clock_out: Option<String>,
    /// 备注；不传 = 不改
    pub remark: Option<String>,
}

/// 导入行：`employeeId` / `userId` 二选一，其余为归一化后的打卡事实。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecordRow {
    /// 员工档案 ID（与 `user_id` 二选一）
    pub employee_id: Option<u64>,
    /// 平台用户 ID（与 `employee_id` 二选一，服务端反查档案）
    pub user_id: Option<u64>,
    /// 出勤日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 上班打卡时间（`yyyy-MM-dd HH:mm:ss`）
    pub clock_in: Option<String>,
    /// 下班打卡时间（`yyyy-MM-dd HH:mm:ss`）
    pub clock_out: Option<String>,
    /// 数据来源：1 导入 2 手工补录 3 设备 4 钉钉 5 飞书
    pub source: i8,
    /// 第三方平台的记录 ID（配合 `source` 判重；本地补录不传）
    pub external_id: Option<String>,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 出勤事实导入请求（归一化行数组；第三方对接只需在适配层映射成本 DTO）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecordReq {
    /// 归一化后的导入行
    pub rows: Vec<ImportRecordRow>,
}

/// 导入错误明细：单行失败不打断整批，错误累积后随回执返回。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecordError {
    /// 行号（从 1 开始，对应请求 `rows` 的下标 + 1）
    pub row: u64,
    /// 失败原因（中文）
    pub message: String,
}

/// 出勤事实导入回执。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecordResp {
    /// 新建的事实行数
    pub created: u64,
    /// 覆盖更新的事实行数
    pub updated: u64,
    /// 因外部记录 ID 冲突而整行跳过的行数
    pub skipped: u64,
    /// 逐行错误明细（`skipped` 行也在此列出原因）
    pub errors: Vec<ImportRecordError>,
}

/// 出勤事实响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordResp {
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 出勤日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 班次 ID 快照；0 = 当天休息
    pub shift_id: u64,
    /// 班次编码（service 批量拼装；班次已软删时留空）
    pub shift_code: String,
    /// 班次名称（service 批量拼装；班次已软删时留空）
    pub shift_name: String,
    /// 上班打卡时间（`yyyy-MM-dd HH:mm:ss`）
    pub clock_in: Option<String>,
    /// 下班打卡时间（`yyyy-MM-dd HH:mm:ss`）
    pub clock_out: Option<String>,
    /// 实际出勤分钟数
    pub actual_minutes: i32,
    /// 迟到分钟数
    pub late_minutes: i32,
    /// 早退分钟数
    pub early_leave_minutes: i32,
    /// 缺卡：0 无 1 缺上班卡 2 缺下班卡 3 上下班卡都缺
    pub miss_clock: i8,
    /// 数据来源：1 导入 2 手工补录 3 设备 4 钉钉 5 飞书
    pub source: i8,
    /// 第三方平台的记录 ID；本地补录为 `None`
    pub external_id: Option<String>,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

/// `hr_attendance_record::Model` → `RecordResp` 字段搬运（人名字段留空待拼装）。
impl From<hr_attendance_record::Model> for RecordResp {
    fn from(m: hr_attendance_record::Model) -> Self {
        Self {
            id: m.id,
            employee_id: m.employee_id,
            employee_name: String::new(),
            work_date: fmt_date(m.work_date),
            shift_id: m.shift_id,
            shift_code: String::new(),
            shift_name: String::new(),
            clock_in: fmt_datetime_opt(m.clock_in),
            clock_out: fmt_datetime_opt(m.clock_out),
            actual_minutes: m.actual_minutes,
            late_minutes: m.late_minutes,
            early_leave_minutes: m.early_leave_minutes,
            miss_clock: m.miss_clock,
            source: m.source,
            external_id: m.external_id,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for RecordResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

// —— 工作日历 `hr_work_calendar` ——

/// 工作日历列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalendarListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 是否工作日精确过滤：1 是 0 否；不传查全部
    pub is_workday: Option<i8>,
    /// 日期类型精确过滤（0 普通 1 法定节假日 2 调休上班）；不传查全部
    pub holiday_type: Option<i8>,
    /// 日期起（`yyyy-MM-dd`）；不传不设下限
    pub date_begin: Option<String>,
    /// 日期止（`yyyy-MM-dd`）；不传不设上限
    pub date_end: Option<String>,
}

/// 工作日历分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct CalendarFilter {
    /// 是否工作日精确过滤
    pub is_workday: Option<i8>,
    /// 日期类型精确过滤
    pub holiday_type: Option<i8>,
    /// 日期起（含）
    pub date_begin: Option<chrono::NaiveDate>,
    /// 日期止（含）
    pub date_end: Option<chrono::NaiveDate>,
}

/// 单日工作日历 upsert 请求（唯一键 `calendar_date`）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertCalendarReq {
    /// 日期（`yyyy-MM-dd`）
    pub calendar_date: String,
    /// 是否工作日：1 是 0 否
    pub is_workday: i8,
    /// 日期类型：0 普通 1 法定节假日 2 调休上班
    pub holiday_type: i8,
    /// 该日标准工时（分钟）：无排班时的折算依据，默认 480
    pub standard_minutes: i32,
    /// 备注（如「国庆节」）
    #[serde(default)]
    pub remark: String,
}

/// 区间工作日历导入请求：日期区间 × 是否工作日 / 类型，逐日 upsert。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchImportCalendarReq {
    /// 区间起（`yyyy-MM-dd`，含）
    pub start_date: String,
    /// 区间止（`yyyy-MM-dd`，含）
    pub end_date: String,
    /// 是否工作日：1 是 0 否
    pub is_workday: i8,
    /// 日期类型：0 普通 1 法定节假日 2 调休上班
    pub holiday_type: i8,
    /// 该日标准工时（分钟）
    pub standard_minutes: i32,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 工作日历导入回执。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchImportCalendarResp {
    /// 新建的日历天数
    pub created: u64,
    /// 更新的日历天数
    pub updated: u64,
}

/// 工作日历响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalendarResp {
    pub id: u64,
    /// 日期（`yyyy-MM-dd`）
    pub calendar_date: String,
    /// 是否工作日：1 是 0 否
    pub is_workday: i8,
    /// 日期类型：0 普通 1 法定节假日 2 调休上班
    pub holiday_type: i8,
    /// 该日标准工时（分钟）
    pub standard_minutes: i32,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

/// `hr_work_calendar::Model` → `CalendarResp` 字段搬运（人名字段留空待拼装）。
impl From<hr_work_calendar::Model> for CalendarResp {
    fn from(m: hr_work_calendar::Model) -> Self {
        Self {
            id: m.id,
            calendar_date: fmt_date(m.calendar_date),
            is_workday: m.is_workday,
            holiday_type: m.holiday_type,
            standard_minutes: m.standard_minutes,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for CalendarResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}
