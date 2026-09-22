//! 考勤域业务：跨域「应出勤窗口」派生 + 班次 / 排班 / 出勤事实 / 工作日历的 CRUD 编排。
//!
//! 分层约定（见 AGENTS.md「分层与事务」）：
//! - 自持事务的入口收 `db: &DatabaseConnection`，内部 `begin` → 委托 `*_in_tx(txn, …)` →
//!   成功即 `commit`；`*_in_tx` 内**不得** begin / commit（事务边界由入口与测试外层事务负责）；
//! - 只读入口收 `&impl ConnectionTrait`，不起事务；
//! - 展示查询一律普通读；本域的写路径是「按唯一键 upsert」，冲突由唯一键兜底
//!   （无「读 → 判断 → 写」的额度式不变式，因此不需要 `SELECT ... FOR UPDATE`）；
//! - 审计字段与软删标记由 repo 盖章；本层只决定业务列与业务文案。
//!
//! # 「应出勤」的口径（跨域冻结契约）
//!
//! 应出勤由「排班 × 日历」实时派生，不落冗余列（排班改动后事实不失真）：
//!
//! | 情形 | `is_workday` | 窗口 | `standard_minutes` |
//! |---|---|---|---|
//! | 当日有排班且 `shift_id > 0` | true | 班次 `start_time`–`end_time`（跨天顺延次日） | 班次 `work_minutes`（已扣休息） |
//! | 当日有排班且 `shift_id = 0` | false | 无 | 0 |
//! | 有排班但引用的班次已被软删 | true | 无（退回标准工作制） | 480 |
//! | 无排班，日历 `is_workday = 1` | true | 整个自然日 | 日历 `standard_minutes` |
//! | 无排班，日历 `is_workday = 0` | false | 无 | 日历 `standard_minutes` |
//! | 无排班且无日历 | true | 整个自然日 | 480（[`DEFAULT_STANDARD_MINUTES`]） |
//!
//! 排班是**权威覆盖**：只要当天有排班行，就不再回看日历（否则「日历说放假、但排了班」会
//! 被日历吃掉）。「排班与日历都查不到按工作日处理」是刻意的兜底——没配排班的公司也不该
//! 把请假时长算成 0。
//!
//! [`derive_work_minutes`] 逐日切分 `[start_at, end_at)` 并与当日窗口求交：
//! - 每个自然日的贡献 = 交集分钟数，**封顶到该日 `standard_minutes`**（班次日即已扣休息的
//!   应工作分钟数，日历日即标准工时）——这样「09:00–18:00 请假、班次含 60 分钟休息」得 480
//!   而不是 540；
//! - 跨天班窗口按**班次开始日**归属：枚举时多带前一天，且上一天跨天班覆盖的凌晨时段不再
//!   计入本日「标准工作制」窗口（否则同 6 小时会被算两次）；
//! - 返回 0 由调用方决定是否报错（请假域报「所选区间内没有应出勤的工作时间」）。

use std::collections::HashMap;

use chrono::{NaiveDateTime, NaiveTime};
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::{hr_attendance_record, hr_shift, hr_shift_schedule, hr_work_calendar};
use crate::modules::biz::hr::attendance::dto::{
    BatchCreateScheduleReq, BatchCreateScheduleResp, BatchImportCalendarReq,
    BatchImportCalendarResp, CalendarFilter, CalendarListReq, CreateShiftReq, ImportRecordError,
    ImportRecordReq, ImportRecordResp, ImportRecordRow, RecordFilter, RecordListReq,
    ScheduleFilter, ScheduleListReq, ScheduleMonthDayResp, ScheduleMonthEmployeeResp,
    ScheduleMonthReq, ScheduleMonthResp, ShiftFilter, ShiftListReq, UpdateRecordReq,
    UpdateScheduleReq, UpdateShiftReq, UpsertCalendarReq,
};
use crate::modules::biz::hr::attendance::{
    DEFAULT_STANDARD_MINUTES, MISS_CLOCK_BOTH, MISS_CLOCK_IN, MISS_CLOCK_NONE, MISS_CLOCK_OUT,
    SCHEDULE_STATUS_UNSCHEDULED, repo,
};
use crate::modules::biz::hr::employee::EMPLOYMENT_STATUS_RESIGNED;
use crate::modules::biz::hr::employee::service::find_employee_name_map;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 一次批量取员工档案的分页大小（`PageQuery` 上限同口径）。
const EMPLOYEE_SCAN_PAGE_SIZE: u64 = 1000;

// —— 冻结契约：跨域「应出勤窗口」派生（`hr/time-off` 与 `hr/overtime` 直接调用）——

/// 某员工某日的应出勤窗口（排班优先，工作日历覆盖）。
#[derive(Debug, Clone)]
pub struct WorkdayWindow {
    /// 当天是否应出勤
    pub is_workday: bool,
    /// 班次 ID；0 = 无排班 / 当天休息
    pub shift_id: u64,
    /// 上班时间（无排班窗口时为 `None`）
    pub start_time: Option<chrono::NaiveTime>,
    /// 下班时间（无排班窗口时为 `None`；`cross_day` 时落在次日）
    pub end_time: Option<chrono::NaiveTime>,
    /// 是否跨天班（`end_time` 落在次日）
    pub cross_day: bool,
    /// 当日折算分钟数：有排班 = 班次应工作分钟数；无排班 = 日历标准工时（默认 480）
    pub standard_minutes: i32,
    /// 迟到宽限分钟数：有班次取班次配置（负值归零）；无班次窗口时为 0
    pub late_tolerance_minutes: i32,
}

/// 某员工某日的应出勤窗口（排班优先，工作日历覆盖）。
///
/// 判定顺序见文件头「应出勤的口径」；本函数是只读查询，可传 `&DatabaseConnection` 或事务。
pub async fn resolve_workday(
    db: &impl sea_orm::ConnectionTrait,
    employee_id: u64,
    work_date: chrono::NaiveDate,
) -> Result<WorkdayWindow, crate::utils::error::AppError> {
    if let Some(schedule) = repo::find_schedule_by_employee_date(db, employee_id, work_date).await?
    {
        // 排班是权威覆盖：显式 `shift_id = 0`（当天休息）不再回看日历
        if schedule.shift_id == 0 {
            return Ok(rest_window(0));
        }
        if let Some(shift) = repo::find_shift_by_id(db, schedule.shift_id).await? {
            return Ok(WorkdayWindow {
                is_workday: true,
                shift_id: shift.id,
                start_time: Some(shift.start_time),
                end_time: Some(shift.end_time),
                cross_day: shift.cross_day == 1,
                standard_minutes: shift.work_minutes.max(0),
                late_tolerance_minutes: shift.late_tolerance_minutes.max(0),
            });
        }
        // 排班引用的班次已被软删：排班本身即「应出勤」信号，退回标准工作制（无窗口）
        return Ok(standard_workday_window(DEFAULT_STANDARD_MINUTES));
    }

    if let Some(calendar) = repo::find_calendar_by_date(db, work_date).await? {
        if calendar.is_workday == 0 {
            return Ok(rest_window(calendar.standard_minutes.max(0)));
        }
        return Ok(standard_workday_window(calendar.standard_minutes.max(0)));
    }

    // 排班与日历都查不到：按工作日处理（未配排班也不至于把时长算成 0）
    Ok(standard_workday_window(DEFAULT_STANDARD_MINUTES))
}

/// 派生「某员工在 `[start_at, end_at)` 之间的应出勤分钟数」：逐自然日切分 × 应出勤窗口 ∩ 区间。
///
/// 返回 0 由调用方决定是否报错（请假域报「所选区间内没有应出勤的工作时间」）。
pub async fn derive_work_minutes(
    db: &impl sea_orm::ConnectionTrait,
    employee_id: u64,
    start_at: chrono::NaiveDateTime,
    end_at: chrono::NaiveDateTime,
) -> Result<i32, crate::utils::error::AppError> {
    if end_at <= start_at {
        return Ok(0);
    }

    let start_date = start_at.date();
    let end_date = end_at.date();
    // 跨天班窗口归属「班次开始日」：多枚举前一天，避免漏掉凌晨那半段
    let mut date = start_date.pred_opt().unwrap_or(start_date);
    let mut previous_cross_end: Option<NaiveDateTime> = None;
    let mut total: i64 = 0;

    loop {
        let window = resolve_workday(db, employee_id, date).await?;
        let (window_start, window_end) = day_window(&window, date);
        // 上一天跨天班已覆盖的凌晨时段不再计入本日「标准工作制」窗口（避免重复计数）
        let day_floor = match (window.shift_id, previous_cross_end) {
            (0, Some(previous_end)) if previous_end.date() == date => previous_end,
            _ => date.and_time(NaiveTime::MIN),
        };
        let window_start = window_start.max(day_floor);

        if window.is_workday {
            let from = start_at.max(window_start);
            let to = end_at.min(window_end);
            if to > from {
                let overlap = (to - from).num_minutes();
                total += overlap.min(i64::from(window.standard_minutes.max(0)));
            }
        }

        previous_cross_end = if window.is_workday && window.shift_id > 0 && window.cross_day {
            Some(window_end)
        } else {
            None
        };

        if date >= end_date {
            break;
        }
        match date.succ_opt() {
            Some(next) => date = next,
            None => break,
        }
    }

    Ok(total.clamp(0, i64::from(i32::MAX)) as i32)
}

/// 当天休息（无窗口）：`standard_minutes` 仅作展示口径。
fn rest_window(standard_minutes: i32) -> WorkdayWindow {
    WorkdayWindow {
        is_workday: false,
        shift_id: 0,
        start_time: None,
        end_time: None,
        cross_day: false,
        standard_minutes,
        late_tolerance_minutes: 0,
    }
}

/// 「标准工作制」工作日窗口（无班次窗口，按整日 × `standard_minutes` 折算）。
fn standard_workday_window(standard_minutes: i32) -> WorkdayWindow {
    WorkdayWindow {
        is_workday: true,
        shift_id: 0,
        start_time: None,
        end_time: None,
        cross_day: false,
        standard_minutes,
        late_tolerance_minutes: 0,
    }
}

/// 取某日的窗口区间 `[start, end)`：有班次用班次窗口（跨天班顺延到次日），无班次取整个自然日。
fn day_window(window: &WorkdayWindow, work_date: Date) -> (NaiveDateTime, NaiveDateTime) {
    let next_date = work_date.succ_opt().unwrap_or(work_date);
    match (window.start_time, window.end_time) {
        (Some(start_time), Some(end_time)) => {
            let end_date = if window.cross_day {
                next_date
            } else {
                work_date
            };
            (work_date.and_time(start_time), end_date.and_time(end_time))
        }
        _ => (
            work_date.and_time(NaiveTime::MIN),
            next_date.and_time(NaiveTime::MIN),
        ),
    }
}

// —— 解析小工具（格式已在 `validate.rs` 拦过，这里是进入 service 后的兜底）——

/// 解析 `yyyy-MM-dd` 日期。
fn parse_date(field: &str, raw: &str) -> Result<Date, AppError> {
    chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::Biz(format!("{field}格式应为 yyyy-MM-dd")))
}

/// 解析可选的日期区间端点（`utils::datetime::parse_datetime` 收 `yyyy-MM-dd` 与完整时间两种）。
fn parse_optional_date(field: &str, raw: &Option<String>) -> Result<Option<Date>, AppError> {
    Ok(crate::utils::datetime::parse_datetime(field, raw, false)?.map(|dt| dt.date()))
}

/// 解析 `HH:MM:SS` 到点时间。
fn parse_time(field: &str, raw: &str) -> Result<NaiveTime, AppError> {
    chrono::NaiveTime::parse_from_str(raw.trim(), "%H:%M:%S")
        .map_err(|_| AppError::Biz(format!("{field}格式应为 HH:MM:SS")))
}

/// 格式化 `yyyy-MM-dd`（月视图网格）。
fn fmt_date(d: Date) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// 分钟差（`to − from`），负值归零、超 `i32` 上限截断。
fn diff_minutes(from: DateTime, to: DateTime) -> i32 {
    (to - from).num_minutes().clamp(0, i64::from(i32::MAX)) as i32
}

// —— 批量取名 / 批量取班次摘要（列表响应回填用；单次查询，禁止逐行查库）——

/// 批量取班次摘要：`hr_shift.id` → (`shift_code`, `shift_name`)（软删班次不出现，调用方留空）。
pub(crate) async fn shift_briefs(
    db: &impl ConnectionTrait,
    shift_ids: &[u64],
) -> Result<HashMap<u64, (String, String)>, AppError> {
    let ids = crate::utils::user_ref::dedup_ids(shift_ids.to_vec());
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let shifts = repo::find_shifts_by_ids(db, &ids).await?;
    Ok(shifts
        .into_iter()
        .map(|s| (s.id, (s.shift_code, s.shift_name)))
        .collect())
}

/// 校验员工档案存在（一次批量查；缺失的 ID 拼进中文错误）。
async fn ensure_employees_exist(
    db: &impl ConnectionTrait,
    employee_ids: &[u64],
) -> Result<(), AppError> {
    if employee_ids.is_empty() {
        return Ok(());
    }

    let employees = crate::modules::biz::hr::employee::repo::find_by_ids(db, employee_ids).await?;
    let found_ids: Vec<u64> = employees.iter().map(|e| e.id).collect();
    let missing = crate::utils::check::collect_missing_ids(employee_ids, found_ids.into_iter());
    if !missing.is_empty() {
        return Err(AppError::Biz(format!(
            "员工档案不存在：{}",
            crate::utils::check::format_ids(&missing)
        )));
    }
    Ok(())
}

/// 校验班次存在（`shift_id = 0` 是「当天休息」的合法值，调用方自行跳过）。
async fn ensure_shift_exists(db: &impl ConnectionTrait, shift_id: u64) -> Result<(), AppError> {
    if repo::find_shift_by_id(db, shift_id).await?.is_none() {
        return Err(AppError::Biz(format!("班次不存在：{shift_id}")));
    }
    Ok(())
}

// —— 班次 CRUD ——

/// 班次分页：请求参数组装为 repo 过滤条件后透传。
pub async fn page_shifts(
    db: &impl ConnectionTrait,
    req: &ShiftListReq,
) -> Result<PageData<hr_shift::Model>, AppError> {
    let filter = ShiftFilter {
        keyword: req.keyword.clone(),
        status: req.status,
    };
    let page =
        repo::find_shift_page(db, &filter, req.page.page_index(), req.page.page_size()).await?;
    Ok(page)
}

/// 创建班次（对外入口）：三行事务，成功后提交。
pub async fn create_shift(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateShiftReq,
) -> Result<hr_shift::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_shift_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内创建班次：编码查重（**含软删占位**）→ 建行。
async fn create_shift_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: CreateShiftReq,
) -> Result<hr_shift::Model, AppError> {
    // 唯一键是单列 `shift_code`，软删行仍占位，所以查重必须看得见软删记录
    if repo::find_shift_by_code_include_deleted(txn, &req.shift_code)
        .await?
        .is_some()
    {
        return Err(AppError::Biz(format!("班次编码已存在：{}", req.shift_code)));
    }

    let model = hr_shift::ActiveModel {
        shift_code: Set(req.shift_code),
        shift_name: Set(req.shift_name),
        start_time: Set(parse_time("上班时间", &req.start_time)?),
        end_time: Set(parse_time("下班时间", &req.end_time)?),
        cross_day: Set(req.cross_day),
        work_minutes: Set(req.work_minutes),
        rest_minutes: Set(req.rest_minutes),
        late_tolerance_minutes: Set(req.late_tolerance_minutes),
        need_clock: Set(req.need_clock),
        status: Set(req.status),
        remark: Set(req.remark),
        ..Default::default()
    };
    Ok(repo::create_shift_in_tx(txn, model, actor_id).await?)
}

/// 更新班次（对外入口）：三行事务，成功后提交。
pub async fn update_shift(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateShiftReq,
) -> Result<hr_shift::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_shift_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内更新班次：存在性 → 编码查重（排除自身）→ 窄写。
async fn update_shift_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateShiftReq,
) -> Result<hr_shift::Model, AppError> {
    if repo::find_shift_by_id(txn, req.id).await?.is_none() {
        return Err(AppError::Biz(format!("班次不存在：{}", req.id)));
    }

    let duplicated = repo::find_shift_by_code_include_deleted(txn, &req.shift_code).await?;
    if duplicated.is_some_and(|model| model.id != req.id) {
        return Err(AppError::Biz(format!("班次编码已存在：{}", req.shift_code)));
    }

    let model = hr_shift::ActiveModel {
        id: Set(req.id),
        shift_code: Set(req.shift_code.clone()),
        shift_name: Set(req.shift_name.clone()),
        start_time: Set(parse_time("上班时间", &req.start_time)?),
        end_time: Set(parse_time("下班时间", &req.end_time)?),
        cross_day: Set(req.cross_day),
        work_minutes: Set(req.work_minutes),
        rest_minutes: Set(req.rest_minutes),
        late_tolerance_minutes: Set(req.late_tolerance_minutes),
        need_clock: Set(req.need_clock),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };
    Ok(repo::update_shift_in_tx(txn, model, actor_id).await?)
}

/// 按 id 查班次详情（软删视为不存在）。
pub async fn get_shift(db: &impl ConnectionTrait, id: u64) -> Result<hr_shift::Model, AppError> {
    repo::find_shift_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("班次不存在：{id}")))
}

/// 删除班次（对外入口）：软删，三行事务，成功后提交。
///
/// 班次被历史排班 / 出勤事实引用**不阻止软删**：软删只标记，老排班仍能按 id 取名
/// （`find_shift_by_id` 不再返回它，但历史快照里的 `shift_id` 依然可追溯）。
pub async fn delete_shift(db: &DatabaseConnection, actor_id: u64, id: u64) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_shift_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内软删班次。
async fn delete_shift_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    if !repo::soft_delete_shift_in_tx(txn, id, actor_id).await? {
        return Err(AppError::Biz(format!("班次不存在：{id}")));
    }
    Ok(())
}

// —— 排班 CRUD ——

/// 排班分页：请求参数组装为 repo 过滤条件后透传。
pub async fn page_schedules(
    db: &impl ConnectionTrait,
    req: &ScheduleListReq,
) -> Result<PageData<hr_shift_schedule::Model>, AppError> {
    let filter = ScheduleFilter {
        employee_id: req.employee_id,
        shift_id: req.shift_id,
        status: req.status,
        work_date_begin: parse_optional_date("排班日起", &req.work_date_begin)?,
        work_date_end: parse_optional_date("排班日止", &req.work_date_end)?,
    };
    let page =
        repo::find_schedule_page(db, &filter, req.page.page_index(), req.page.page_size()).await?;
    Ok(page)
}

/// 批量排班（对外入口）：三行事务，成功后提交。
pub async fn batch_create_schedules(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &BatchCreateScheduleReq,
) -> Result<BatchCreateScheduleResp, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = batch_create_schedules_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内批量排班：员工 × 日期区间逐日 upsert，回执新建 / 更新计数。
///
/// 幂等：重复调用同一批参数，第二次全部命中已有行 → `created = 0`、`updated = 员工数 × 天数`。
async fn batch_create_schedules_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &BatchCreateScheduleReq,
) -> Result<BatchCreateScheduleResp, AppError> {
    let start = parse_date("开始日期", &req.start_date)?;
    let end = parse_date("结束日期", &req.end_date)?;
    if end < start {
        return Err(AppError::Biz("结束日期不能早于开始日期".to_string()));
    }

    let employee_ids = crate::utils::user_ref::dedup_ids(req.employee_ids.clone());
    ensure_employees_exist(txn, &employee_ids).await?;
    if req.shift_id > 0 {
        ensure_shift_exists(txn, req.shift_id).await?;
    }

    let mut created = 0u64;
    let mut updated = 0u64;
    let mut date = start;
    loop {
        for employee_id in &employee_ids {
            match repo::find_schedule_by_employee_date(txn, *employee_id, date).await? {
                Some(existing) => {
                    repo::update_schedule_in_tx(
                        txn,
                        hr_shift_schedule::ActiveModel {
                            id: Set(existing.id),
                            shift_id: Set(req.shift_id),
                            status: Set(req.status),
                            remark: Set(req.remark.clone()),
                            ..Default::default()
                        },
                        actor_id,
                    )
                    .await?;
                    updated += 1;
                }
                None => {
                    repo::create_schedule_in_tx(
                        txn,
                        hr_shift_schedule::ActiveModel {
                            employee_id: Set(*employee_id),
                            work_date: Set(date),
                            shift_id: Set(req.shift_id),
                            status: Set(req.status),
                            remark: Set(req.remark.clone()),
                            ..Default::default()
                        },
                        actor_id,
                    )
                    .await?;
                    created += 1;
                }
            }
        }
        if date >= end {
            break;
        }
        match date.succ_opt() {
            Some(next) => date = next,
            None => break,
        }
    }

    Ok(BatchCreateScheduleResp { created, updated })
}

/// 单日排班 upsert（对外入口）：三行事务，成功后提交。
pub async fn update_schedule(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateScheduleReq,
) -> Result<hr_shift_schedule::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_schedule_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内单日排班 upsert：按唯一键 `(employee_id, work_date)` 定位，命中即更新，否则新建。
async fn update_schedule_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateScheduleReq,
) -> Result<hr_shift_schedule::Model, AppError> {
    let work_date = parse_date("排班日期", &req.work_date)?;
    ensure_employees_exist(txn, &[req.employee_id]).await?;
    if req.shift_id > 0 {
        ensure_shift_exists(txn, req.shift_id).await?;
    }

    match repo::find_schedule_by_employee_date(txn, req.employee_id, work_date).await? {
        Some(existing) => Ok(repo::update_schedule_in_tx(
            txn,
            hr_shift_schedule::ActiveModel {
                id: Set(existing.id),
                shift_id: Set(req.shift_id),
                status: Set(req.status),
                remark: Set(req.remark.clone()),
                ..Default::default()
            },
            actor_id,
        )
        .await?),
        None => Ok(repo::create_schedule_in_tx(
            txn,
            hr_shift_schedule::ActiveModel {
                employee_id: Set(req.employee_id),
                work_date: Set(work_date),
                shift_id: Set(req.shift_id),
                status: Set(req.status),
                remark: Set(req.remark.clone()),
                ..Default::default()
            },
            actor_id,
        )
        .await?),
    }
}

/// 排班月视图：某月 × 某部门 / 全员，一行一个员工、`days` 覆盖该月每一天。
///
/// 未排班的日子用 `status = SCHEDULE_STATUS_UNSCHEDULED`（0）占位，**不落库**——
/// 与「当天休息」（`shift_id = 0` 且 `status = 1`）可区分。
pub async fn month_schedules(
    db: &impl ConnectionTrait,
    req: &ScheduleMonthReq,
) -> Result<ScheduleMonthResp, AppError> {
    let month = req.month.trim();
    let first = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| AppError::Biz("月份格式应为 yyyy-MM".to_string()))?;
    let next_first = first
        .checked_add_months(chrono::Months::new(1))
        .ok_or_else(|| AppError::Biz("月份超出可表示范围".to_string()))?;
    let last = next_first.pred_opt().unwrap_or(next_first);

    let employee_ids = resolve_scope_employee_ids(db, req.dept_id).await?;
    let schedules =
        repo::find_schedules_by_employee_ids_and_range(db, &employee_ids, first, last).await?;
    let shift_ids: Vec<u64> = schedules
        .iter()
        .map(|s| s.shift_id)
        .filter(|id| *id > 0)
        .collect();
    let briefs = shift_briefs(db, &shift_ids).await?;
    let names = find_employee_name_map(db, &employee_ids).await?;

    let mut by_key: HashMap<(u64, Date), &hr_shift_schedule::Model> = HashMap::new();
    for schedule in &schedules {
        by_key.insert((schedule.employee_id, schedule.work_date), schedule);
    }

    let mut employees = Vec::with_capacity(employee_ids.len());
    for employee_id in employee_ids {
        let mut days = Vec::new();
        let mut date = first;
        while date <= last {
            let mut day = ScheduleMonthDayResp {
                work_date: fmt_date(date),
                shift_id: 0,
                shift_code: String::new(),
                shift_name: String::new(),
                status: SCHEDULE_STATUS_UNSCHEDULED,
                remark: String::new(),
            };
            if let Some(schedule) = by_key.get(&(employee_id, date)) {
                day.shift_id = schedule.shift_id;
                day.status = schedule.status;
                day.remark = schedule.remark.clone();
                if let Some((code, name)) = briefs.get(&schedule.shift_id) {
                    day.shift_code = code.clone();
                    day.shift_name = name.clone();
                }
            }
            days.push(day);
            match date.succ_opt() {
                Some(next) => date = next,
                None => break,
            }
        }
        employees.push(ScheduleMonthEmployeeResp {
            employee_id,
            employee_name: names.get(&employee_id).cloned().unwrap_or_default(),
            days,
        });
    }

    Ok(ScheduleMonthResp {
        month: month.to_string(),
        employees,
    })
}

/// 解析月视图的员工范围：给 `dept_id` 取该部门账号挂载的档案，否则全员（排除离职）。
async fn resolve_scope_employee_ids(
    db: &impl ConnectionTrait,
    dept_id: Option<u64>,
) -> Result<Vec<u64>, AppError> {
    use crate::modules::biz::hr::employee::dto::EmployeeFilter;
    use crate::modules::biz::hr::employee::repo as employee_repo;

    if let Some(dept_id) = dept_id {
        let user_ids =
            crate::modules::system::user::repo::find_user_ids_by_dept_id(db, dept_id).await?;
        let employees = employee_repo::find_by_user_ids(db, &user_ids).await?;
        let employee_by_user: HashMap<u64, u64> =
            employees.into_iter().map(|e| (e.user_id, e.id)).collect();
        // 按 user_ids 顺序回填，保序且自然去重
        return Ok(crate::utils::user_ref::dedup_ids(
            user_ids
                .into_iter()
                .filter_map(|user_id| employee_by_user.get(&user_id).copied())
                .collect(),
        ));
    }

    let mut ids = Vec::new();
    let mut page_index = 0u64;
    loop {
        let page = employee_repo::find_employee_page(
            db,
            &EmployeeFilter::default(),
            page_index,
            EMPLOYEE_SCAN_PAGE_SIZE,
        )
        .await?;
        let fetched = page.items.len() as u64;
        ids.extend(
            page.items
                .into_iter()
                .filter(|employee| employee.employment_status != EMPLOYMENT_STATUS_RESIGNED)
                .map(|employee| employee.id),
        );
        if fetched == 0 || (page_index + 1) * EMPLOYEE_SCAN_PAGE_SIZE >= page.total {
            break;
        }
        page_index += 1;
    }
    Ok(crate::utils::user_ref::dedup_ids(ids))
}

// —— 出勤事实 ——

/// 出勤事实分页：请求参数组装为 repo 过滤条件后透传。
pub async fn page_records(
    db: &impl ConnectionTrait,
    req: &RecordListReq,
) -> Result<PageData<hr_attendance_record::Model>, AppError> {
    let filter = RecordFilter {
        employee_id: req.employee_id,
        source: req.source,
        miss_clock: req.miss_clock,
        work_date_begin: parse_optional_date("出勤日起", &req.work_date_begin)?,
        work_date_end: parse_optional_date("出勤日止", &req.work_date_end)?,
    };
    let page =
        repo::find_record_page(db, &filter, req.page.page_index(), req.page.page_size()).await?;
    Ok(page)
}

/// 按 id 查出勤事实详情。
pub async fn get_record(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_attendance_record::Model, AppError> {
    repo::find_record_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("出勤记录不存在：{id}")))
}

/// 手工补录 / 修正出勤事实（对外入口）：三行事务，成功后提交。
pub async fn update_record(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateRecordReq,
) -> Result<hr_attendance_record::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_record_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内手工补录：按当前排班口径重算派生列并刷新班次快照。
///
/// `clock_in` / `clock_out` 为 `None` 表示不改；空串表示清空（`parse_datetime` 把空串视为 `None`）。
/// 先后**按合并后的结果**校验：只给一端时另一端取库中值，`clock_out < clock_in` 直接报
/// `下班打卡时间不能早于上班打卡时间`（只按请求体校验会落出「无缺卡却出勤 0」的自相矛盾记录）。
async fn update_record_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateRecordReq,
) -> Result<hr_attendance_record::Model, AppError> {
    let existing = repo::find_record_by_id(txn, req.id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("出勤记录不存在：{}", req.id)))?;

    let clock_in = if req.clock_in.is_some() {
        crate::utils::datetime::parse_datetime("上班打卡时间", &req.clock_in, false)?
    } else {
        existing.clock_in
    };
    let clock_out = if req.clock_out.is_some() {
        crate::utils::datetime::parse_datetime("下班打卡时间", &req.clock_out, false)?
    } else {
        existing.clock_out
    };

    // 合并后的先后必须自洽：只改一端时，另一端来自库中值，仅按请求体校验会落出 `clock_in > clock_out`
    if let (Some(from), Some(to)) = (clock_in, clock_out)
        && to < from
    {
        return Err(AppError::Biz(
            "下班打卡时间不能早于上班打卡时间".to_string(),
        ));
    }

    let window = resolve_workday(txn, existing.employee_id, existing.work_date).await?;
    let tolerance = window.late_tolerance_minutes;
    let derived = derive_attendance(&window, existing.work_date, clock_in, clock_out, tolerance);

    let mut model = hr_attendance_record::ActiveModel {
        id: Set(existing.id),
        shift_id: Set(window.shift_id),
        clock_in: Set(clock_in),
        clock_out: Set(clock_out),
        actual_minutes: Set(derived.actual_minutes),
        late_minutes: Set(derived.late_minutes),
        early_leave_minutes: Set(derived.early_leave_minutes),
        miss_clock: Set(derived.miss_clock),
        ..Default::default()
    };
    if let Some(remark) = &req.remark {
        model.remark = Set(remark.clone());
    }

    Ok(repo::update_record_in_tx(txn, model, actor_id).await?)
}

/// 出勤事实导入（对外入口）：**整批一个事务**，单行业务失败只累积错误、不打断整批。
pub async fn import_records(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &ImportRecordReq,
) -> Result<ImportRecordResp, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = import_records_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内导入：逐行按 `(employee_id, work_date)` upsert，回执 `created` / `updated` /
/// `skipped` 与逐行错误。
///
/// 单行失败（员工定位不到、日期格式错、外部记录 ID 被他人占用）只记一条错误并继续下一行；
/// 数据库层失败（唯一键竞争等）仍会整批回滚——那是真并发异常，不该被当成「单行失败」吞掉。
async fn import_records_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &ImportRecordReq,
) -> Result<ImportRecordResp, AppError> {
    let (employee_ids, user_ids) = collect_import_refs(&req.rows);
    let known_employees: HashMap<u64, u64> =
        crate::modules::biz::hr::employee::repo::find_by_ids(txn, &employee_ids)
            .await?
            .into_iter()
            .map(|e| (e.id, e.id))
            .collect();
    let employee_by_user: HashMap<u64, u64> =
        crate::modules::biz::hr::employee::repo::find_by_user_ids(txn, &user_ids)
            .await?
            .into_iter()
            .map(|e| (e.user_id, e.id))
            .collect();

    let mut created = 0u64;
    let mut updated = 0u64;
    let mut skipped = 0u64;
    let mut errors: Vec<ImportRecordError> = Vec::new();

    for (index, row) in req.rows.iter().enumerate() {
        let row_no = index as u64 + 1;

        // ① 员工定位：`employeeId` / `userId` 二选一
        let employee_id = match (row.employee_id, row.user_id) {
            (Some(id), None) => {
                if known_employees.contains_key(&id) {
                    id
                } else {
                    errors.push(ImportRecordError {
                        row: row_no,
                        message: format!("员工档案不存在：{id}"),
                    });
                    continue;
                }
            }
            (None, Some(user_id)) => match employee_by_user.get(&user_id) {
                Some(id) => *id,
                None => {
                    errors.push(ImportRecordError {
                        row: row_no,
                        message: format!("userId 对应的员工档案不存在：{user_id}"),
                    });
                    continue;
                }
            },
            _ => {
                errors.push(ImportRecordError {
                    row: row_no,
                    message: "employeeId 与 userId 必须二选一".to_string(),
                });
                continue;
            }
        };

        // ② 日期与打卡时间
        let work_date = match parse_date("workDate", &row.work_date) {
            Ok(date) => date,
            Err(_) => {
                errors.push(ImportRecordError {
                    row: row_no,
                    message: format!("workDate 格式应为 yyyy-MM-dd：{}", row.work_date),
                });
                continue;
            }
        };
        let clock_in = match crate::utils::datetime::parse_datetime("clockIn", &row.clock_in, false)
        {
            Ok(value) => value,
            Err(_) => {
                errors.push(ImportRecordError {
                    row: row_no,
                    message: "clockIn 格式应为 yyyy-MM-dd HH:mm:ss".to_string(),
                });
                continue;
            }
        };
        let clock_out =
            match crate::utils::datetime::parse_datetime("clockOut", &row.clock_out, false) {
                Ok(value) => value,
                Err(_) => {
                    errors.push(ImportRecordError {
                        row: row_no,
                        message: "clockOut 格式应为 yyyy-MM-dd HH:mm:ss".to_string(),
                    });
                    continue;
                }
            };

        // ③ 外部记录 ID 判重：命中同 `(source, external_id)` 且不是同一员工 / 日期 → 记错误跳过
        if let Some(external_id) = non_empty(&row.external_id)
            && external_id_taken_by_other(txn, row.source, external_id, employee_id, work_date)
                .await?
        {
            skipped += 1;
            errors.push(ImportRecordError {
                row: row_no,
                message: format!("外部记录 ID 已被其他记录占用：{external_id}"),
            });
            continue;
        }

        // ④ 迟到 / 早退 / 实际出勤 / 缺卡派生（无排班日只落原始打卡时间）
        let window = resolve_workday(txn, employee_id, work_date).await?;
        let tolerance = window.late_tolerance_minutes;
        let derived = derive_attendance(&window, work_date, clock_in, clock_out, tolerance);

        // ⑤ 按唯一键 `(employee_id, work_date)` upsert（行是权威口径，覆盖来源与外部 ID）。
        // 空串外部 ID 归为 `None` 落 `NULL`：判重读取侧本就把空串当「无外部 ID」，落 `''`
        // 会占住 `(source, external_id)` 唯一键，让后续同批行绕过判重后撞唯一键整批回滚。
        match repo::find_record_by_employee_date(txn, employee_id, work_date).await? {
            Some(existing) => {
                repo::update_record_in_tx(
                    txn,
                    hr_attendance_record::ActiveModel {
                        id: Set(existing.id),
                        shift_id: Set(window.shift_id),
                        clock_in: Set(clock_in),
                        clock_out: Set(clock_out),
                        actual_minutes: Set(derived.actual_minutes),
                        late_minutes: Set(derived.late_minutes),
                        early_leave_minutes: Set(derived.early_leave_minutes),
                        miss_clock: Set(derived.miss_clock),
                        source: Set(row.source),
                        external_id: Set(non_empty(&row.external_id).map(str::to_owned)),
                        remark: Set(row.remark.clone()),
                        ..Default::default()
                    },
                    actor_id,
                )
                .await?;
                updated += 1;
            }
            None => {
                repo::create_record_in_tx(
                    txn,
                    hr_attendance_record::ActiveModel {
                        employee_id: Set(employee_id),
                        work_date: Set(work_date),
                        shift_id: Set(window.shift_id),
                        clock_in: Set(clock_in),
                        clock_out: Set(clock_out),
                        actual_minutes: Set(derived.actual_minutes),
                        late_minutes: Set(derived.late_minutes),
                        early_leave_minutes: Set(derived.early_leave_minutes),
                        miss_clock: Set(derived.miss_clock),
                        source: Set(row.source),
                        external_id: Set(non_empty(&row.external_id).map(str::to_owned)),
                        remark: Set(row.remark.clone()),
                        ..Default::default()
                    },
                    actor_id,
                )
                .await?;
                created += 1;
            }
        }
    }

    Ok(ImportRecordResp {
        created,
        updated,
        skipped,
        errors,
    })
}

/// 收集导入行里的员工 / 用户 ID（各自去重），供一次批量查档案。
fn collect_import_refs(rows: &[ImportRecordRow]) -> (Vec<u64>, Vec<u64>) {
    let employee_ids: Vec<u64> = rows.iter().filter_map(|row| row.employee_id).collect();
    let user_ids: Vec<u64> = rows.iter().filter_map(|row| row.user_id).collect();
    (
        crate::utils::user_ref::dedup_ids(employee_ids),
        crate::utils::user_ref::dedup_ids(user_ids),
    )
}

/// 取非空（trim 后）的外部记录 ID。
fn non_empty(raw: &Option<String>) -> Option<&str> {
    raw.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// 同 `(source, external_id)` 的记录是否已属于「别的员工 / 别的日期」。
///
/// 命中同一员工同一天不算冲突——那是同一份事实的重复推送，走 upsert 覆盖即可。
async fn external_id_taken_by_other(
    db: &impl ConnectionTrait,
    source: i8,
    external_id: &str,
    employee_id: u64,
    work_date: Date,
) -> Result<bool, AppError> {
    let existing = repo::find_record_by_source_external_id(db, source, external_id).await?;
    Ok(existing
        .is_some_and(|record| record.employee_id != employee_id || record.work_date != work_date))
}

/// 派生后的出勤结果（迟到 / 早退 / 实际出勤 / 缺卡）。
struct DerivedAttendance {
    actual_minutes: i32,
    late_minutes: i32,
    early_leave_minutes: i32,
    miss_clock: i8,
}

/// 由打卡时间与当日窗口派生考勤结果。
///
/// **只有挂在班次上的应出勤日才派生迟到 / 早退 / 缺卡**（无排班日没有可比对的窗口，按规格
/// 只落原始打卡时间，`actual_minutes` 退化为两次打卡的差值）；迟到按
/// `late_tolerance_minutes` 宽限后取正，早退不设宽限。
fn derive_attendance(
    window: &WorkdayWindow,
    work_date: Date,
    clock_in: Option<DateTime>,
    clock_out: Option<DateTime>,
    late_tolerance_minutes: i32,
) -> DerivedAttendance {
    let raw_pair = match (clock_in, clock_out) {
        (Some(from), Some(to)) if to > from => diff_minutes(from, to),
        _ => 0,
    };

    if !window.is_workday || window.shift_id == 0 {
        return DerivedAttendance {
            actual_minutes: raw_pair,
            late_minutes: 0,
            early_leave_minutes: 0,
            miss_clock: MISS_CLOCK_NONE,
        };
    }

    let miss_clock = match (clock_in.is_some(), clock_out.is_some()) {
        (true, true) => MISS_CLOCK_NONE,
        (true, false) => MISS_CLOCK_OUT,
        (false, true) => MISS_CLOCK_IN,
        (false, false) => MISS_CLOCK_BOTH,
    };

    let (window_start, window_end) = day_window(window, work_date);
    let late_minutes = match clock_in {
        Some(from) => (diff_minutes(window_start, from) - late_tolerance_minutes.max(0)).max(0),
        None => 0,
    };
    let early_leave_minutes = match clock_out {
        Some(to) => diff_minutes(to, window_end),
        None => 0,
    };
    let actual_minutes = match (clock_in, clock_out) {
        (Some(from), Some(to)) => {
            let from = from.max(window_start);
            let to = to.min(window_end);
            if to > from {
                diff_minutes(from, to).min(window.standard_minutes.max(0))
            } else {
                0
            }
        }
        _ => 0,
    };

    DerivedAttendance {
        actual_minutes,
        late_minutes,
        early_leave_minutes,
        miss_clock,
    }
}

// —— 工作日历 ——

/// 工作日历分页：请求参数组装为 repo 过滤条件后透传。
pub async fn page_calendars(
    db: &impl ConnectionTrait,
    req: &CalendarListReq,
) -> Result<PageData<hr_work_calendar::Model>, AppError> {
    let filter = CalendarFilter {
        is_workday: req.is_workday,
        holiday_type: req.holiday_type,
        date_begin: parse_optional_date("日期起", &req.date_begin)?,
        date_end: parse_optional_date("日期止", &req.date_end)?,
    };
    let page =
        repo::find_calendar_page(db, &filter, req.page.page_index(), req.page.page_size()).await?;
    Ok(page)
}

/// 单日工作日历 upsert（对外入口）：三行事务，成功后提交。
pub async fn upsert_calendar(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpsertCalendarReq,
) -> Result<hr_work_calendar::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = upsert_calendar_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内单日 upsert：按唯一键 `calendar_date` 定位，命中即更新，否则新建。
async fn upsert_calendar_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpsertCalendarReq,
) -> Result<hr_work_calendar::Model, AppError> {
    let calendar_date = parse_date("日期", &req.calendar_date)?;

    match repo::find_calendar_by_date(txn, calendar_date).await? {
        Some(existing) => Ok(repo::update_calendar_in_tx(
            txn,
            hr_work_calendar::ActiveModel {
                id: Set(existing.id),
                is_workday: Set(req.is_workday),
                holiday_type: Set(req.holiday_type),
                standard_minutes: Set(req.standard_minutes),
                remark: Set(req.remark.clone()),
                ..Default::default()
            },
            actor_id,
        )
        .await?),
        None => Ok(repo::create_calendar_in_tx(
            txn,
            hr_work_calendar::ActiveModel {
                calendar_date: Set(calendar_date),
                is_workday: Set(req.is_workday),
                holiday_type: Set(req.holiday_type),
                standard_minutes: Set(req.standard_minutes),
                remark: Set(req.remark.clone()),
                ..Default::default()
            },
            actor_id,
        )
        .await?),
    }
}

/// 区间工作日历导入（对外入口）：三行事务，逐日 upsert，成功后提交。
pub async fn batch_import_calendars(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &BatchImportCalendarReq,
) -> Result<BatchImportCalendarResp, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = batch_import_calendars_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内逐日 upsert 日历（幂等：重复导入同区间第二次全部走 update）。
async fn batch_import_calendars_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &BatchImportCalendarReq,
) -> Result<BatchImportCalendarResp, AppError> {
    let start = parse_date("开始日期", &req.start_date)?;
    let end = parse_date("结束日期", &req.end_date)?;
    if end < start {
        return Err(AppError::Biz("结束日期不能早于开始日期".to_string()));
    }

    let mut created = 0u64;
    let mut updated = 0u64;
    let mut date = start;
    loop {
        match repo::find_calendar_by_date(txn, date).await? {
            Some(existing) => {
                repo::update_calendar_in_tx(
                    txn,
                    hr_work_calendar::ActiveModel {
                        id: Set(existing.id),
                        is_workday: Set(req.is_workday),
                        holiday_type: Set(req.holiday_type),
                        standard_minutes: Set(req.standard_minutes),
                        remark: Set(req.remark.clone()),
                        ..Default::default()
                    },
                    actor_id,
                )
                .await?;
                updated += 1;
            }
            None => {
                repo::create_calendar_in_tx(
                    txn,
                    hr_work_calendar::ActiveModel {
                        calendar_date: Set(date),
                        is_workday: Set(req.is_workday),
                        holiday_type: Set(req.holiday_type),
                        standard_minutes: Set(req.standard_minutes),
                        remark: Set(req.remark.clone()),
                        ..Default::default()
                    },
                    actor_id,
                )
                .await?;
                created += 1;
            }
        }
        if date >= end {
            break;
        }
        match date.succ_opt() {
            Some(next) => date = next,
            None => break,
        }
    }

    Ok(BatchImportCalendarResp { created, updated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::hr_employee;
    use crate::modules::biz::hr::attendance::dto::ImportRecordRow;
    use crate::modules::biz::hr::attendance::{
        HOLIDAY_TYPE_STATUTORY, SCHEDULE_STATUS_NORMAL, SOURCE_IMPORT, SOURCE_MANUAL,
    };
    use sea_orm::{ActiveModelTrait, Database, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同进程内并行用例（repo / service 两个模块）必须互不相同，
    /// 否则撞 `uk_hr_shift_code`。
    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一员工 ID：段位 900_5xx，与 repo 测试（900_4xx）、time_off（900_2xx / 900_3xx）
    /// 错开，避免同进程并行撞 `uk_hr_employee_user_id`。
    fn unique_employee_id() -> u64 {
        900_500_000
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed) % 1_000
    }

    /// 唯一日历日期：按年分桶（各用例用各自的年，避免同进程并行时撞 `uk_hr_work_calendar_date`），
    /// 远离真实业务日期。
    fn unique_calendar_date(year: i32) -> Date {
        let offset = SEQ.fetch_add(1, Ordering::Relaxed) % 360;
        chrono::NaiveDate::from_ymd_opt(year, 1, 1)
            .unwrap()
            .checked_add_days(chrono::Days::new(offset))
            .unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> Date {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    fn time(h: u32, m: u32, s: u32) -> NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, s).unwrap()
    }

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> DatabaseTransaction {
        test_db().await.begin().await.unwrap()
    }

    /// 直插一份员工档案，返回 `hr_employee.id`。
    async fn seed_employee(txn: &DatabaseTransaction) -> u64 {
        hr_employee::ActiveModel {
            user_id: Set(unique_employee_id() + 10_000),
            employment_status: Set(1),
            education: Set(0),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap()
        .id
    }

    /// 直插一个班次（`shift_code` 唯一），返回 `hr_shift.id`。
    async fn seed_shift(
        txn: &DatabaseTransaction,
        start_time: NaiveTime,
        end_time: NaiveTime,
        cross_day: i8,
        work_minutes: i32,
        late_tolerance_minutes: i32,
    ) -> u64 {
        repo::create_shift_in_tx(
            txn,
            hr_shift::ActiveModel {
                shift_code: Set(unique("att_service")),
                shift_name: Set("测试班次".to_owned()),
                start_time: Set(start_time),
                end_time: Set(end_time),
                cross_day: Set(cross_day),
                work_minutes: Set(work_minutes),
                rest_minutes: Set(0),
                late_tolerance_minutes: Set(late_tolerance_minutes),
                need_clock: Set(1),
                status: Set(1),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap()
        .id
    }

    /// 直插一条排班，返回 `hr_shift_schedule.id`。
    async fn seed_schedule(
        txn: &DatabaseTransaction,
        employee_id: u64,
        work_date: Date,
        shift_id: u64,
    ) -> u64 {
        repo::create_schedule_in_tx(
            txn,
            hr_shift_schedule::ActiveModel {
                employee_id: Set(employee_id),
                work_date: Set(work_date),
                shift_id: Set(shift_id),
                status: Set(SCHEDULE_STATUS_NORMAL),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn resolve_workday_takes_the_shift_window_from_the_schedule() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let shift_id = seed_shift(&txn, time(9, 0, 0), time(18, 0, 0), 0, 480, 5).await;
        let work_date = date(2096, 9, 7);
        seed_schedule(&txn, employee_id, work_date, shift_id).await;

        let window = resolve_workday(&txn, employee_id, work_date).await.unwrap();
        assert!(window.is_workday, "排了班就必须是应出勤日");
        assert_eq!(window.shift_id, shift_id, "必须回带班次 ID");
        assert_eq!(
            window.start_time,
            Some(time(9, 0, 0)),
            "窗口起点取班次上班时间"
        );
        assert_eq!(
            window.end_time,
            Some(time(18, 0, 0)),
            "窗口终点取班次下班时间"
        );
        assert!(!window.cross_day, "非跨天班不得标跨天");
        assert_eq!(window.standard_minutes, 480, "标准分钟数取班次应工作分钟数");
    }

    #[tokio::test]
    async fn resolve_workday_treats_an_explicit_rest_schedule_as_non_workday() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let work_date = unique_calendar_date(2095);
        // `shift_id = 0` = 当天休息：排班是权威覆盖，即便日历说是工作日也不算应出勤
        repo::create_calendar_in_tx(
            &txn,
            hr_work_calendar::ActiveModel {
                calendar_date: Set(work_date),
                is_workday: Set(1),
                holiday_type: Set(0),
                standard_minutes: Set(480),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();
        seed_schedule(&txn, employee_id, work_date, 0).await;

        let window = resolve_workday(&txn, employee_id, work_date).await.unwrap();
        assert!(!window.is_workday, "显式排休息必须是非工作日");
        assert_eq!(window.shift_id, 0, "休息日无班次");
    }

    #[tokio::test]
    async fn resolve_workday_falls_back_to_the_calendar_when_there_is_no_schedule() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let holiday = unique_calendar_date(2097);
        repo::create_calendar_in_tx(
            &txn,
            hr_work_calendar::ActiveModel {
                calendar_date: Set(holiday),
                is_workday: Set(0),
                holiday_type: Set(HOLIDAY_TYPE_STATUTORY),
                standard_minutes: Set(0),
                remark: Set("法定节假日".to_owned()),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();

        let window = resolve_workday(&txn, employee_id, holiday).await.unwrap();
        assert!(!window.is_workday, "日历 is_workday = 0 必须是非工作日");
        assert_eq!(window.shift_id, 0, "无排班时班次 ID 为 0");
        assert!(window.start_time.is_none(), "无排班窗口");
        assert_eq!(window.standard_minutes, 0, "标准分钟数取日历值");

        let next_day = holiday.succ_opt().unwrap();
        repo::create_calendar_in_tx(
            &txn,
            hr_work_calendar::ActiveModel {
                calendar_date: Set(next_day),
                is_workday: Set(1),
                holiday_type: Set(2),
                standard_minutes: Set(300),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();
        let workday = resolve_workday(&txn, employee_id, next_day).await.unwrap();
        assert!(workday.is_workday, "日历 is_workday = 1 必须是工作日");
        assert_eq!(workday.shift_id, 0, "无排班时班次 ID 为 0");
        assert_eq!(
            workday.standard_minutes, 300,
            "标准分钟数必须取日历 standard_minutes"
        );
    }

    #[tokio::test]
    async fn resolve_workday_defaults_to_a_workday_when_schedule_and_calendar_are_missing() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        // 2026-11-11：既未排班也未配日历（两个测试模块都不会占用该日期）
        let window = resolve_workday(&txn, employee_id, date(2026, 11, 11))
            .await
            .unwrap();
        assert!(window.is_workday, "排班与日历都缺时必须按工作日处理");
        assert_eq!(window.shift_id, 0);
        assert_eq!(
            window.standard_minutes, DEFAULT_STANDARD_MINUTES,
            "默认标准工时必须是 480 分钟"
        );
    }

    #[tokio::test]
    async fn derive_work_minutes_counts_only_the_half_day_overlap_on_a_shift_day() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let shift_id = seed_shift(&txn, time(9, 0, 0), time(18, 0, 0), 0, 480, 5).await;
        let work_date = date(2026, 9, 9);
        seed_schedule(&txn, employee_id, work_date, shift_id).await;

        let morning = derive_work_minutes(
            &txn,
            employee_id,
            at(2026, 9, 9, 9, 0),
            at(2026, 9, 9, 12, 0),
        )
        .await
        .unwrap();
        assert_eq!(morning, 180, "上午半日必须只算窗口内交集 180 分钟");

        let full = derive_work_minutes(
            &txn,
            employee_id,
            at(2026, 9, 9, 9, 0),
            at(2026, 9, 9, 18, 0),
        )
        .await
        .unwrap();
        assert_eq!(full, 480, "整窗请假必须封顶到班次应工作分钟数（已扣休息）");

        let outside = derive_work_minutes(
            &txn,
            employee_id,
            at(2026, 9, 9, 20, 0),
            at(2026, 9, 9, 22, 0),
        )
        .await
        .unwrap();
        assert_eq!(outside, 0, "窗口之外不计入应出勤时长");
    }

    #[tokio::test]
    async fn derive_work_minutes_attributes_a_cross_day_shift_to_its_start_date() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let shift_id = seed_shift(&txn, time(22, 0, 0), time(6, 0, 0), 1, 480, 0).await;
        let work_date = date(2026, 9, 10);
        seed_schedule(&txn, employee_id, work_date, shift_id).await;

        let whole_shift = derive_work_minutes(
            &txn,
            employee_id,
            at(2026, 9, 10, 22, 0),
            at(2026, 9, 11, 6, 0),
        )
        .await
        .unwrap();
        assert_eq!(whole_shift, 480, "整段跨天班必须算满 480 分钟");

        // 只请次日凌晨那一半：必须归属班次开始日，且不得被次日默认工作日重复计数
        let tail = derive_work_minutes(
            &txn,
            employee_id,
            at(2026, 9, 11, 0, 0),
            at(2026, 9, 11, 6, 0),
        )
        .await
        .unwrap();
        assert_eq!(tail, 360, "凌晨 6 小时只算一次，不得被次日窗口重复计入");
    }

    #[tokio::test]
    async fn batch_create_schedules_is_idempotent_and_reports_created_then_updated() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let shift_id = seed_shift(&txn, time(9, 0, 0), time(18, 0, 0), 0, 480, 0).await;
        let req = BatchCreateScheduleReq {
            employee_ids: vec![employee_id],
            start_date: "2026-09-14".to_string(),
            end_date: "2026-09-16".to_string(),
            shift_id,
            status: SCHEDULE_STATUS_NORMAL,
            remark: "排班".to_string(),
        };

        let first = batch_create_schedules_in_tx(&txn, ACTOR_ID, &req)
            .await
            .unwrap();
        assert_eq!(first.created, 3, "三天应新建三行");
        assert_eq!(first.updated, 0, "首次排班没有可更新的行");

        let second = batch_create_schedules_in_tx(&txn, ACTOR_ID, &req)
            .await
            .unwrap();
        assert_eq!(second.created, 0, "重复排班不得新建行（唯一键 upsert）");
        assert_eq!(second.updated, 3, "重复排班必须命中已有行并更新");

        let page = page_schedules(
            &txn,
            &ScheduleListReq {
                page: crate::utils::PageQuery {
                    page: Some(1),
                    page_size: Some(10),
                },
                employee_id: Some(employee_id),
                shift_id: None,
                status: None,
                work_date_begin: Some("2026-09-14".to_string()),
                work_date_end: Some("2026-09-16".to_string()),
            },
        )
        .await
        .unwrap();
        assert_eq!(page.total, 3, "重复排班不得产生重复行");
    }

    #[tokio::test]
    async fn import_records_reports_created_updated_skipped_and_keeps_going_on_row_errors() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let other_employee_id = seed_employee(&txn).await;
        let shift_id = seed_shift(&txn, time(9, 0, 0), time(18, 0, 0), 0, 480, 5).await;

        // 另一名员工已占用 (source=1, externalId=E)：第三行带同一外部 ID 必须被跳过
        let taken = unique("att_ext");
        seed_schedule(&txn, employee_id, date(2026, 9, 21), shift_id).await;
        seed_schedule(&txn, other_employee_id, date(2026, 9, 21), shift_id).await;
        repo::create_record_in_tx(
            &txn,
            hr_attendance_record::ActiveModel {
                employee_id: Set(other_employee_id),
                work_date: Set(date(2026, 9, 21)),
                source: Set(SOURCE_IMPORT),
                external_id: Set(Some(taken.clone())),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();

        // 第 1 行：新建（迟到 20 分钟 - 宽限 5 分钟 = 15 分钟迟到）
        let first = ImportRecordRow {
            employee_id: Some(employee_id),
            user_id: None,
            work_date: "2026-09-21".to_string(),
            clock_in: Some("2026-09-21 09:20:00".to_string()),
            clock_out: Some("2026-09-21 18:00:00".to_string()),
            source: SOURCE_IMPORT,
            external_id: None,
            remark: String::new(),
        };
        // 第 2 行：员工不存在 → 单行错误，不打断整批
        let missing_employee = ImportRecordRow {
            employee_id: Some(employee_id + 900_000_000),
            user_id: None,
            work_date: "2026-09-21".to_string(),
            clock_in: None,
            clock_out: None,
            source: SOURCE_MANUAL,
            external_id: None,
            remark: String::new(),
        };
        // 第 3 行：外部记录 ID 被他人占用 → skipped + 错误
        let conflicted = ImportRecordRow {
            employee_id: Some(employee_id),
            user_id: None,
            work_date: "2026-09-21".to_string(),
            clock_in: None,
            clock_out: None,
            source: SOURCE_IMPORT,
            external_id: Some(taken.clone()),
            remark: String::new(),
        };
        // 第 4 行：同一员工同一日期再导入 → 覆盖更新
        let overwrite = ImportRecordRow {
            employee_id: Some(employee_id),
            user_id: None,
            work_date: "2026-09-21".to_string(),
            clock_in: Some("2026-09-21 09:00:00".to_string()),
            clock_out: Some("2026-09-21 18:00:00".to_string()),
            source: SOURCE_MANUAL,
            external_id: None,
            remark: "手工补录".to_string(),
        };

        let resp = import_records_in_tx(
            &txn,
            ACTOR_ID,
            &ImportRecordReq {
                rows: vec![first, missing_employee, conflicted, overwrite],
            },
        )
        .await
        .unwrap();

        assert_eq!(resp.created, 1, "第一次导入该员工该日应新建一行");
        assert_eq!(resp.updated, 1, "同员工同日的第二行应覆盖更新，不再新建");
        assert_eq!(resp.skipped, 1, "外部记录 ID 被占用的行必须跳过");
        assert_eq!(
            resp.errors.len(),
            2,
            "员工不存在与外部 ID 冲突各记一条错误（单行失败不打断整批）"
        );
        assert!(
            resp.errors.iter().all(|e| e.row > 0),
            "错误必须带从 1 开始的行号"
        );

        let record = repo::find_record_by_employee_date(&txn, employee_id, date(2026, 9, 21))
            .await
            .unwrap()
            .expect("导入后必须能按 (employee_id, work_date) 查到事实");
        assert_eq!(
            record.source, SOURCE_MANUAL,
            "最后一次导入的来源必须覆盖旧值"
        );
        assert_eq!(record.remark, "手工补录", "备注必须随最后一次导入落库");
        assert_eq!(
            record.clock_in,
            Some(at(2026, 9, 21, 9, 0)),
            "打卡时间必须落库"
        );
        assert_eq!(record.late_minutes, 0, "09:00 打卡不迟到");
        assert_eq!(record.shift_id, shift_id, "必须落班次快照");
        assert_eq!(record.miss_clock, MISS_CLOCK_NONE, "两次打卡都不缺");
    }

    /// 适配层把「没有外部记录 ID」映射为空串时，导入必须按「无外部 ID」处理（落 `NULL`）。
    ///
    /// 空串落库会占住 `uk_hr_attendance_record_external(source, external_id)`：判重读取把空串
    /// 当无外部 ID，第二行绕过判重直达 INSERT 撞唯一键 → 整批回滚；且该键被 `''` 永久占据。
    #[tokio::test]
    async fn import_treats_empty_external_id_as_null() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;

        let rows = [date(2026, 10, 1), date(2026, 10, 2)]
            .into_iter()
            .map(|day| ImportRecordRow {
                employee_id: Some(employee_id),
                user_id: None,
                work_date: day.format("%Y-%m-%d").to_string(),
                clock_in: Some(format!("{} 09:00:00", day.format("%Y-%m-%d"))),
                clock_out: Some(format!("{} 18:00:00", day.format("%Y-%m-%d"))),
                source: SOURCE_IMPORT,
                external_id: Some(String::new()),
                remark: String::new(),
            })
            .collect::<Vec<_>>();

        let resp = import_records_in_tx(&txn, ACTOR_ID, &ImportRecordReq { rows })
            .await
            .unwrap();
        assert_eq!(resp.created, 2, "两行空串外部 ID 都必须导入成功");
        assert_eq!(resp.errors.len(), 0, "不得有行级错误");

        for day in [date(2026, 10, 1), date(2026, 10, 2)] {
            let record = repo::find_record_by_employee_date(&txn, employee_id, day)
                .await
                .unwrap()
                .expect("两行都必须落库");
            assert!(
                record.external_id.is_none(),
                "空串外部 ID 必须落 NULL（否则占用唯一键并让后续导入整批失败）"
            );
        }
    }

    /// 手工补录只改一个打卡时间时，必须按**合并后**的时间校验先后：
    /// 库里已有 18:00 下班，只把上班改成 19:00 应被拒（否则出现「无缺卡 + 出勤 0 + 迟到 595」的自相矛盾记录）。
    #[tokio::test]
    async fn update_record_rejects_inverted_clock_after_merge() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let day = date(2026, 10, 3);
        let record = repo::create_record_in_tx(
            &txn,
            hr_attendance_record::ActiveModel {
                employee_id: Set(employee_id),
                work_date: Set(day),
                clock_in: Set(Some(at(2026, 10, 3, 9, 0))),
                clock_out: Set(Some(at(2026, 10, 3, 18, 0))),
                source: Set(SOURCE_MANUAL),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();

        let err = update_record_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRecordReq {
                id: record.id,
                clock_in: Some("2026-10-03 19:00:00".to_string()),
                clock_out: None,
                remark: None,
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("不能早于"),
            "合并后 clock_in > clock_out 必须被拒，实际：{err}"
        );
    }
}
