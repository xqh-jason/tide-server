//! 考勤请求体校验（纯函数，无第三方校验框架）。
//!
//! 结构同 employee / time_off / position 域：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求
//! 提供一个 `validate_xxx(req, …) -> Result<(), String>`，命中规则即累积中文错误消息，多条用
//! 「；」拼接后返回（可读性优于只回首条）。handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! 班次 `status` 的允许值唯一来源是平台字典（`dictionary::service::enabled_int_values(&db, "status")`），
//! 由调用方预取后作为 `status_allowed` 传入，**不硬编码**；文案直接用 `utils::check::check_status`
//! 的返回（「状态取值不合法，仅允许：…」），不自己造句。
//!
//! 排班状态（1 正常 / 2 已换班）、出勤来源、缺卡、日历类型是**本域自有的枚举**（不在平台 `status`
//! 字典里），值域来自 `mod.rs` 的常量数组（仍是唯一定义点）。
//!
//! 需要查库的规则（`shift_code` 查重、班次 / 员工存在性、外部记录 ID 冲突）留在 service 层。

use crate::modules::biz::hr::attendance::dto::{
    BatchCreateScheduleReq, BatchImportCalendarReq, CreateShiftReq, ImportRecordReq,
    UpdateRecordReq, UpdateScheduleReq, UpdateShiftReq, UpsertCalendarReq,
};
use crate::modules::biz::hr::attendance::{HOLIDAY_TYPES, SCHEDULE_STATUSES, SOURCES};
use crate::utils::check;

/// 班次编码长度上限（对齐 `VARCHAR(32)`）。
const SHIFT_CODE_MAX: usize = 32;
/// 班次名称长度上限（对齐 `VARCHAR(64)`）。
const SHIFT_NAME_MAX: usize = 64;
/// 备注长度上限（对齐 `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;
/// 第三方外部记录 ID 长度上限（对齐 `VARCHAR(64)`）。
const EXTERNAL_ID_MAX: usize = 64;
/// 单次出勤事实导入行数上限。
const MAX_IMPORT_ROWS: usize = 1000;
/// 单次批量排班人数上限。
const MAX_BATCH_EMPLOYEES: usize = 1000;
/// 单次批量排班的「人数 × 天数」总行数上限：单事务逐行「探测读 + 写入」，超限必然超时并整体回滚。
const MAX_BATCH_SCHEDULE_ROWS: u64 = 10_000;
/// 单次排班 / 日历导入的区间天数上限（跨自然日，含两端）。
const MAX_RANGE_DAYS: i64 = 366;
/// 单日标准工时上限（分钟）——一天最多 24 小时。
const MAX_STANDARD_MINUTES: i32 = 1440;

/// 创建 / 更新班次共用的字段视图（两个 DTO 除 `id` 外字段同名同型，抽视图避免复制粘贴）。
struct ShiftFields<'a> {
    shift_code: &'a str,
    shift_name: &'a str,
    start_time: &'a str,
    end_time: &'a str,
    cross_day: i8,
    work_minutes: i32,
    rest_minutes: i32,
    late_tolerance_minutes: i32,
    need_clock: i8,
    status: i8,
    remark: &'a str,
}

/// 必填文本：trim 后非空 + 长度上限（按 `char` 计，避免 UTF-8 边界误判）。
fn check_required(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.trim().is_empty() {
        errors.push(format!("{label}不能为空"));
    } else if s.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 值域检查：`{label}取值不合法，仅允许：a / b`。
fn check_int_in(value: i8, allowed: &[i8], label: &str, errors: &mut Vec<String>) {
    if allowed.contains(&value) {
        return;
    }
    let choices = allowed
        .iter()
        .map(i8::to_string)
        .collect::<Vec<_>>()
        .join(" / ");
    errors.push(format!("{label}取值不合法，仅允许：{choices}"));
}

/// 班次编码字符集：只允许小写字母 / 数字 / 下划线。
fn check_code_charset(code: &str, errors: &mut Vec<String>) {
    let code = code.trim();
    if code.is_empty() {
        return;
    }
    let ok = code
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        errors.push("班次编码只能包含小写字母、数字与下划线".to_string());
    }
}

/// 解析 `yyyy-MM-dd`；格式错记一条 `{label}格式应为 yyyy-MM-dd`。
fn parse_date(raw: &str, label: &str, errors: &mut Vec<String>) -> Option<chrono::NaiveDate> {
    match chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d") {
        Ok(date) => Some(date),
        Err(_) => {
            errors.push(format!("{label}格式应为 yyyy-MM-dd"));
            None
        }
    }
}

/// 解析 `HH:MM:SS`；格式错记一条 `{label}格式应为 HH:MM:SS`。
fn parse_time(raw: &str, label: &str, errors: &mut Vec<String>) -> Option<chrono::NaiveTime> {
    match chrono::NaiveTime::parse_from_str(raw.trim(), "%H:%M:%S") {
        Ok(time) => Some(time),
        Err(_) => {
            errors.push(format!("{label}格式应为 HH:MM:SS"));
            None
        }
    }
}

/// 可选打卡时间：`None` / 空串视为「未提供 / 清空」（合法），格式错记一条中文提示。
fn parse_clock(
    raw: &Option<String>,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<chrono::NaiveDateTime> {
    let value = raw.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    match chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        Ok(datetime) => Some(datetime),
        Err(_) => {
            errors.push(format!("{label}格式应为 yyyy-MM-dd HH:mm:ss"));
            None
        }
    }
}

/// 日期区间检查（起止都解析成功才继续）：顺序 + 跨度上限。
fn check_date_range(
    start: Option<chrono::NaiveDate>,
    end: Option<chrono::NaiveDate>,
    max_days: i64,
    span_label: &str,
    errors: &mut Vec<String>,
) {
    let (Some(start), Some(end)) = (start, end) else {
        return;
    };
    if end < start {
        errors.push("结束日期不能早于开始日期".to_string());
    } else if (end - start).num_days() + 1 > max_days {
        errors.push(format!("{span_label}不能超过 {max_days} 天"));
    }
}

/// 累积的错误用中文分号拼成一条消息。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 班次窗口分钟数：非跨天班为 `end − start`；跨天班下班时间顺延次日（`end + 24h − start`）。
fn window_minutes(start: chrono::NaiveTime, end: chrono::NaiveTime, cross_day: i8) -> i64 {
    let base = (end - start).num_minutes();
    if cross_day == 1 { base + 24 * 60 } else { base }
}

/// 创建 / 更新班次共用字段检查。
fn check_shift_fields(fields: ShiftFields<'_>, status_allowed: &[i8]) -> Vec<String> {
    let mut errors = Vec::new();

    check_required(fields.shift_code, "班次编码", SHIFT_CODE_MAX, &mut errors);
    check_code_charset(fields.shift_code, &mut errors);
    check_required(fields.shift_name, "班次名称", SHIFT_NAME_MAX, &mut errors);

    let start = parse_time(fields.start_time, "上班时间", &mut errors);
    let end = parse_time(fields.end_time, "下班时间", &mut errors);

    check_int_in(fields.cross_day, &[0, 1], "是否跨天班", &mut errors);
    if fields.work_minutes <= 0 {
        errors.push("应工作分钟数必须大于 0".to_string());
    }
    if fields.work_minutes > MAX_STANDARD_MINUTES {
        errors.push(format!("应工作分钟数不能超过 {MAX_STANDARD_MINUTES} 分钟"));
    }
    if fields.rest_minutes < 0 {
        errors.push("休息分钟数不能小于 0".to_string());
    }
    if fields.late_tolerance_minutes < 0 {
        errors.push("迟到宽限分钟数不能小于 0".to_string());
    }
    check_int_in(fields.need_clock, &[0, 1], "是否需要打卡", &mut errors);
    if fields.remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }

    // 起止时间都能解析时再判跨天口径是否自洽
    if let (Some(start), Some(end)) = (start, end) {
        if fields.cross_day == 0 && end <= start {
            errors.push("非跨天班的下班时间必须晚于上班时间".to_string());
        } else if fields.cross_day == 1 && end > start {
            // `end == start` 是 24 小时窗口，合法；只有「晚于」才是交叉区间
            errors.push("跨天班的下班时间不得晚于上班时间".to_string());
        }
    }

    // 工时 + 休息必须落在班次窗口内，否则 `derive_work_minutes` 的逐日封顶值（= `work_minutes`）
    // 会大于真实窗口，请假时长与加班校验一起畸变
    if let (Some(start), Some(end)) = (start, end)
        && matches!(fields.cross_day, 0 | 1)
    {
        let window = window_minutes(start, end, fields.cross_day);
        if i64::from(fields.work_minutes) + i64::from(fields.rest_minutes) > window {
            errors.push("应工作分钟数与休息分钟数之和不能超过班次窗口".to_string());
        }
    }

    // 状态值域的唯一来源是平台字典，文案用 check_status 的返回，不自己造句
    check::check_status(fields.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();

    errors
}

/// 创建班次校验。
///
/// 规则（命中即累积进 `Vec<String>`，最后 `errors.join("；")`；下面文案是契约，测试断言子串）：
/// - `shift_code`：trim 后非空 → `班次编码不能为空`；长度 > 32（按 `char`）→
///   `班次编码长度不能超过 32 个字符`；仅允许小写字母 / 数字 / 下划线 →
///   `班次编码只能包含小写字母、数字与下划线`；
/// - `shift_name`：trim 后非空 → `班次名称不能为空`；长度 > 64 → `班次名称长度不能超过 64 个字符`；
/// - `start_time` / `end_time` 非 `HH:MM:SS` → `上班时间格式应为 HH:MM:SS` / `下班时间格式应为 HH:MM:SS`；
/// - `cross_day` ∉ {0, 1} → `是否跨天班取值不合法，仅允许：0 / 1`；
///   非跨天班 `end <= start` → `非跨天班的下班时间必须晚于上班时间`；
///   跨天班 `end > start` → `跨天班的下班时间不得晚于上班时间`（`end == start` 是 24 小时窗口，合法）；
/// - `work_minutes <= 0` → `应工作分钟数必须大于 0`；`work_minutes > 1440` →
///   `应工作分钟数不能超过 1440 分钟`；`rest_minutes < 0` → `休息分钟数不能小于 0`；
///   `late_tolerance_minutes < 0` → `迟到宽限分钟数不能小于 0`；
/// - 起止时间都能解析且 `cross_day ∈ {0, 1}` 时，`work_minutes + rest_minutes` 大于窗口分钟数
///   （非跨天班窗口 `end − start`，跨天班顺延次日 `end + 24h − start`）→
///   `应工作分钟数与休息分钟数之和不能超过班次窗口`；
/// - `need_clock` ∉ {0, 1} → `是否需要打卡取值不合法，仅允许：0 / 1`；
/// - `remark` 长度 > 255 → `备注长度不能超过 255 个字符`；
/// - `status`：`check::check_status(req.status, status_allowed)` 的 `Err` 原样入列。
pub fn validate_create_shift(req: &CreateShiftReq, status_allowed: &[i8]) -> Result<(), String> {
    let errors = check_shift_fields(
        ShiftFields {
            shift_code: &req.shift_code,
            shift_name: &req.shift_name,
            start_time: &req.start_time,
            end_time: &req.end_time,
            cross_day: req.cross_day,
            work_minutes: req.work_minutes,
            rest_minutes: req.rest_minutes,
            late_tolerance_minutes: req.late_tolerance_minutes,
            need_clock: req.need_clock,
            status: req.status,
            remark: &req.remark,
        },
        status_allowed,
    );
    join_errors(errors)
}

/// 更新班次校验：规则同创建，另加主键检查。
///
/// 追加规则：`id` = 0 → `班次 ID 必须大于 0`；其余字段文案与 [`validate_create_shift`] 逐字一致。
pub fn validate_update_shift(req: &UpdateShiftReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("班次 ID 必须大于 0".to_string());
    }
    errors.extend(check_shift_fields(
        ShiftFields {
            shift_code: &req.shift_code,
            shift_name: &req.shift_name,
            start_time: &req.start_time,
            end_time: &req.end_time,
            cross_day: req.cross_day,
            work_minutes: req.work_minutes,
            rest_minutes: req.rest_minutes,
            late_tolerance_minutes: req.late_tolerance_minutes,
            need_clock: req.need_clock,
            status: req.status,
            remark: &req.remark,
        },
        status_allowed,
    ));
    join_errors(errors)
}

/// 批量排班校验。
///
/// 规则：员工列表为空 → `员工列表不能为空`；去重后 > 1000 → `单次排班人数不能超过 1000`；
/// `start_date` / `end_date` 非 `yyyy-MM-dd` → `开始日期格式应为 yyyy-MM-dd` /
/// `结束日期格式应为 yyyy-MM-dd`；止早于起 → `结束日期不能早于开始日期`；
/// 跨度 > 366 天 → `排班区间不能超过 366 天`；两个日期都能解析且去重后人数 > 0 时，
/// `人数 × 天数` > 10000 → `单次排班总行数不能超过 10000`（「人数 ≤ 1000」与「区间 ≤ 366 天」
/// 各自成立时乘积仍可到 36 万行，而 service 在同一事务里逐行「探测读 + 写入」必然超时回滚）；
/// `status` ∉ {1, 2} → 值域错误；`remark` > 255 → 长度错误。`shift_id = 0` 是「排为休息」的
/// 合法值，不校验班次存在性（存在性需要查库，留在 service）。
pub fn validate_batch_create_schedules(req: &BatchCreateScheduleReq) -> Result<(), String> {
    let mut errors = Vec::new();

    let employee_count = crate::utils::user_ref::dedup_ids(req.employee_ids.clone()).len();
    if req.employee_ids.is_empty() {
        errors.push("员工列表不能为空".to_string());
    }
    if employee_count > MAX_BATCH_EMPLOYEES {
        errors.push(format!("单次排班人数不能超过 {MAX_BATCH_EMPLOYEES}"));
    }
    check_int_in(req.status, &SCHEDULE_STATUSES, "排班状态", &mut errors);
    if req.remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }

    let start = parse_date(&req.start_date, "开始日期", &mut errors);
    let end = parse_date(&req.end_date, "结束日期", &mut errors);
    check_date_range(start, end, MAX_RANGE_DAYS, "排班区间", &mut errors);

    // 「人数 × 天数」总量上限：单量上限都成立也拦不住 36 万行的乘积，必须在入口挡掉
    if let (Some(start), Some(end)) = (start, end)
        && end >= start
        && employee_count > 0
    {
        let days = (end - start).num_days() as u64 + 1;
        if (employee_count as u64).saturating_mul(days) > MAX_BATCH_SCHEDULE_ROWS {
            errors.push(format!("单次排班总行数不能超过 {MAX_BATCH_SCHEDULE_ROWS}"));
        }
    }

    join_errors(errors)
}

/// 单日排班 upsert 校验。
///
/// 规则：`employee_id` = 0 → `员工 ID 必须大于 0`；`work_date` 非 `yyyy-MM-dd` → 格式错误；
/// `status` ∉ {1, 2} → 值域错误；`remark` > 255 → 长度错误。
pub fn validate_update_schedule(req: &UpdateScheduleReq) -> Result<(), String> {
    let mut errors = Vec::new();

    if req.employee_id == 0 {
        errors.push("员工 ID 必须大于 0".to_string());
    }
    parse_date(&req.work_date, "排班日期", &mut errors);
    check_int_in(req.status, &SCHEDULE_STATUSES, "排班状态", &mut errors);
    if req.remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }

    join_errors(errors)
}

/// 手工补录 / 修正出勤事实校验。
///
/// 规则：`id` = 0 → `出勤记录 ID 必须大于 0`；打卡时间非空时必须是 `yyyy-MM-dd HH:mm:ss`
/// → `上班打卡时间格式应为 yyyy-MM-dd HH:mm:ss` / `下班打卡时间格式应为 yyyy-MM-dd HH:mm:ss`；
/// 两者都能解析且下班早于上班 → `下班打卡时间不能早于上班打卡时间`；
/// `remark` > 255 → 长度错误。
pub fn validate_update_record(req: &UpdateRecordReq) -> Result<(), String> {
    let mut errors = Vec::new();

    if req.id == 0 {
        errors.push("出勤记录 ID 必须大于 0".to_string());
    }
    let clock_in = parse_clock(&req.clock_in, "上班打卡时间", &mut errors);
    let clock_out = parse_clock(&req.clock_out, "下班打卡时间", &mut errors);
    if let (Some(clock_in), Some(clock_out)) = (clock_in, clock_out)
        && clock_out < clock_in
    {
        errors.push("下班打卡时间不能早于上班打卡时间".to_string());
    }
    if let Some(remark) = &req.remark
        && remark.chars().count() > REMARK_MAX
    {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }

    join_errors(errors)
}

/// 出勤事实导入校验（逐行累积）。
///
/// 规则：行数为空 → `导入行不能为空`；> 1000 → `单次导入行数不能超过 1000`；逐行检查
/// `employeeId` / `userId` 二选一、`workDate` 格式、`source` ∈ {1..5}、打卡时间格式与先后、
/// 备注与外部 ID 长度，文案以 `第 N 行` 前缀定位（如 `第 2 行：employeeId 与 userId 必须二选一`）。
/// **单行失败不阻断其余行**——与 service 的「错误累积」口径一致。
pub fn validate_import_records(req: &ImportRecordReq) -> Result<(), String> {
    let mut errors = Vec::new();

    if req.rows.is_empty() {
        errors.push("导入行不能为空".to_string());
    }
    if req.rows.len() > MAX_IMPORT_ROWS {
        errors.push(format!("单次导入行数不能超过 {MAX_IMPORT_ROWS}"));
    }

    for (index, row) in req.rows.iter().enumerate() {
        let no = index + 1;

        // `employeeId` / `userId` 恰好二选一（都缺与都给都是语义错误）
        if matches!(
            (row.employee_id, row.user_id),
            (Some(_), Some(_)) | (None, None)
        ) {
            errors.push(format!("第 {no} 行：employeeId 与 userId 必须二选一"));
        }

        parse_date(&row.work_date, &format!("第 {no} 行日期"), &mut errors);
        check_int_in(
            row.source,
            &SOURCES,
            &format!("第 {no} 行出勤来源"),
            &mut errors,
        );

        if row.remark.chars().count() > REMARK_MAX {
            errors.push(format!("第 {no} 行：备注长度不能超过 {REMARK_MAX} 个字符"));
        }
        if let Some(external_id) = &row.external_id
            && external_id.chars().count() > EXTERNAL_ID_MAX
        {
            errors.push(format!(
                "第 {no} 行：外部记录 ID 长度不能超过 {EXTERNAL_ID_MAX} 个字符"
            ));
        }

        let clock_in = parse_clock(
            &row.clock_in,
            &format!("第 {no} 行上班打卡时间"),
            &mut errors,
        );
        let clock_out = parse_clock(
            &row.clock_out,
            &format!("第 {no} 行下班打卡时间"),
            &mut errors,
        );
        if let (Some(clock_in), Some(clock_out)) = (clock_in, clock_out)
            && clock_out < clock_in
        {
            errors.push(format!("第 {no} 行：下班打卡时间不能早于上班打卡时间"));
        }
    }

    join_errors(errors)
}

/// 单日工作日历 upsert 校验。
///
/// 规则：`calendar_date` 非 `yyyy-MM-dd` → `日期格式应为 yyyy-MM-dd`；
/// `is_workday` ∉ {0, 1} → 值域错误；`holiday_type` ∉ {0, 1, 2} → 值域错误；
/// `standard_minutes` ≤ 0 或 > 1440 → `标准工时必须大于 0 且不超过 1440 分钟`；
/// `remark` > 255 → 长度错误。
pub fn validate_upsert_calendar(req: &UpsertCalendarReq) -> Result<(), String> {
    let mut errors = Vec::new();

    parse_date(&req.calendar_date, "日期", &mut errors);
    check_int_in(req.is_workday, &[0, 1], "是否工作日", &mut errors);
    check_int_in(req.holiday_type, &HOLIDAY_TYPES, "日期类型", &mut errors);
    if req.standard_minutes <= 0 || req.standard_minutes > MAX_STANDARD_MINUTES {
        errors.push(format!(
            "标准工时必须大于 0 且不超过 {MAX_STANDARD_MINUTES} 分钟"
        ));
    }
    if req.remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }

    join_errors(errors)
}

/// 区间工作日历导入校验。
///
/// 规则同 [`validate_upsert_calendar`] 的字段部分，另加日期区间检查（格式、顺序、跨度 ≤ 366 天）。
pub fn validate_batch_import_calendars(req: &BatchImportCalendarReq) -> Result<(), String> {
    let mut errors = Vec::new();

    check_int_in(req.is_workday, &[0, 1], "是否工作日", &mut errors);
    check_int_in(req.holiday_type, &HOLIDAY_TYPES, "日期类型", &mut errors);
    if req.standard_minutes <= 0 || req.standard_minutes > MAX_STANDARD_MINUTES {
        errors.push(format!(
            "标准工时必须大于 0 且不超过 {MAX_STANDARD_MINUTES} 分钟"
        ));
    }
    if req.remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }

    let start = parse_date(&req.start_date, "开始日期", &mut errors);
    let end = parse_date(&req.end_date, "结束日期", &mut errors);
    check_date_range(start, end, MAX_RANGE_DAYS, "导入区间", &mut errors);

    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::biz::hr::attendance::dto::ImportRecordRow;
    use crate::modules::biz::hr::attendance::{SOURCE_IMPORT, SOURCE_MANUAL};

    /// `status` 允许值由平台字典提供（`check_status` 的唯一来源），测试用通用 0 / 1 档。
    const STATUS_ALLOWED: &[i8] = &[0, 1];

    /// 合法班次请求基线：各用例只改被测字段，其余保持合法（避免副作用误报）。
    fn valid_shift_req() -> CreateShiftReq {
        CreateShiftReq {
            shift_code: "day_shift".to_string(),
            shift_name: "白班".to_string(),
            start_time: "09:00:00".to_string(),
            end_time: "18:00:00".to_string(),
            cross_day: 0,
            work_minutes: 480,
            rest_minutes: 60,
            late_tolerance_minutes: 5,
            need_clock: 1,
            status: 1,
            remark: String::new(),
        }
    }

    /// 合法导入行基线。
    fn valid_import_row(work_date: &str) -> ImportRecordRow {
        ImportRecordRow {
            employee_id: Some(1),
            user_id: None,
            work_date: work_date.to_string(),
            clock_in: None,
            clock_out: None,
            source: SOURCE_IMPORT,
            external_id: None,
            remark: String::new(),
        }
    }

    #[test]
    fn valid_shift_request_passes_validation() {
        assert!(
            validate_create_shift(&valid_shift_req(), STATUS_ALLOWED).is_ok(),
            "基线请求必须通过校验"
        );
    }

    #[test]
    fn shift_validation_accumulates_chinese_messages_for_each_bad_field() {
        let req = CreateShiftReq {
            shift_code: "Day Shift".to_string(),
            shift_name: String::new(),
            start_time: "9:00".to_string(),
            end_time: "18:00:00".to_string(),
            cross_day: 0,
            work_minutes: 0,
            rest_minutes: -1,
            late_tolerance_minutes: -1,
            need_clock: 2,
            status: 7,
            remark: String::new(),
        };
        let message = validate_create_shift(&req, STATUS_ALLOWED).unwrap_err();

        assert!(
            message.contains("班次编码只能包含小写字母、数字与下划线"),
            "编码字符集错误必须提示：{message}"
        );
        assert!(message.contains("班次名称不能为空"), "名称必填：{message}");
        assert!(
            message.contains("上班时间格式应为 HH:MM:SS"),
            "时间格式错误必须提示：{message}"
        );
        assert!(
            message.contains("应工作分钟数必须大于 0"),
            "工作分钟数必须为正：{message}"
        );
        assert!(message.contains("休息分钟数不能小于 0"), "{message}");
        assert!(message.contains("迟到宽限分钟数不能小于 0"), "{message}");
        assert!(
            message.contains("是否需要打卡取值不合法，仅允许：0 / 1"),
            "{message}"
        );
        assert!(
            message.contains("状态取值不合法"),
            "状态值域必须走字典口径：{message}"
        );
        assert!(
            message.contains("；"),
            "多条错误必须用中文分号拼接：{message}"
        );
    }

    #[test]
    fn shift_validation_rejects_inconsistent_cross_day() {
        let mut req = valid_shift_req();
        req.cross_day = 1;
        // 跨天班却给出「晚于上班时间」的下班时间
        let message = validate_create_shift(&req, STATUS_ALLOWED).unwrap_err();
        assert!(
            message.contains("跨天班的下班时间不得晚于上班时间"),
            "{message}"
        );

        let mut req = valid_shift_req();
        req.end_time = "09:00:00".to_string();
        let message = validate_create_shift(&req, STATUS_ALLOWED).unwrap_err();
        assert!(
            message.contains("非跨天班的下班时间必须晚于上班时间"),
            "{message}"
        );
    }

    #[test]
    fn update_shift_requires_a_positive_id() {
        let mut req = valid_shift_req();
        let update = UpdateShiftReq {
            id: 0,
            shift_code: req.shift_code.clone(),
            shift_name: req.shift_name.clone(),
            start_time: req.start_time.clone(),
            end_time: req.end_time.clone(),
            cross_day: req.cross_day,
            work_minutes: req.work_minutes,
            rest_minutes: req.rest_minutes,
            late_tolerance_minutes: req.late_tolerance_minutes,
            need_clock: req.need_clock,
            status: req.status,
            remark: std::mem::take(&mut req.remark),
        };
        let message = validate_update_shift(&update, STATUS_ALLOWED).unwrap_err();
        assert!(message.contains("班次 ID 必须大于 0"), "{message}");
    }

    #[test]
    fn batch_schedule_validation_requires_employees_and_ordered_dates() {
        let req = BatchCreateScheduleReq {
            employee_ids: Vec::new(),
            start_date: "2026-09-16".to_string(),
            end_date: "2026-09-14".to_string(),
            shift_id: 0,
            status: 1,
            remark: String::new(),
        };
        let message = validate_batch_create_schedules(&req).unwrap_err();
        assert!(message.contains("员工列表不能为空"), "{message}");
        assert!(message.contains("结束日期不能早于开始日期"), "{message}");

        let too_long = BatchCreateScheduleReq {
            employee_ids: vec![1],
            start_date: "2026-01-01".to_string(),
            end_date: "2027-01-02".to_string(),
            shift_id: 0,
            status: 1,
            remark: String::new(),
        };
        let message = validate_batch_create_schedules(&too_long).unwrap_err();
        assert!(message.contains("排班区间不能超过 366 天"), "{message}");
    }

    #[test]
    fn import_validation_locates_each_bad_row_without_stopping_the_batch() {
        let mut bad_date = valid_import_row("2026/09/21");
        bad_date.source = 9;
        let both_ids = ImportRecordRow {
            employee_id: Some(1),
            user_id: Some(2),
            work_date: "2026-09-21".to_string(),
            clock_in: Some("2026-09-21 18:00:00".to_string()),
            clock_out: Some("2026-09-21 09:00:00".to_string()),
            source: SOURCE_MANUAL,
            external_id: None,
            remark: String::new(),
        };
        let req = ImportRecordReq {
            rows: vec![valid_import_row("2026-09-20"), bad_date, both_ids],
        };
        let message = validate_import_records(&req).unwrap_err();

        assert!(
            message.contains("第 2 行日期格式应为 yyyy-MM-dd"),
            "{message}"
        );
        assert!(message.contains("第 2 行出勤来源取值不合法"), "{message}");
        assert!(
            message.contains("第 3 行：employeeId 与 userId 必须二选一"),
            "{message}"
        );
        assert!(
            message.contains("第 3 行：下班打卡时间不能早于上班打卡时间"),
            "{message}"
        );
        assert!(
            !message.contains("第 1 行"),
            "合法行不得产生错误：{message}"
        );
    }

    #[test]
    fn import_validation_rejects_more_than_one_thousand_rows() {
        let rows = (0..1001).map(|_| valid_import_row("2026-09-21")).collect();
        let message = validate_import_records(&ImportRecordReq { rows }).unwrap_err();
        assert!(message.contains("单次导入行数不能超过 1000"), "{message}");
    }

    #[test]
    fn calendar_validation_enforces_workday_type_and_standard_minutes() {
        let req = UpsertCalendarReq {
            calendar_date: "2026-10-01".to_string(),
            is_workday: 2,
            holiday_type: 5,
            standard_minutes: 0,
            remark: String::new(),
        };
        let message = validate_upsert_calendar(&req).unwrap_err();
        assert!(
            message.contains("是否工作日取值不合法，仅允许：0 / 1"),
            "{message}"
        );
        assert!(
            message.contains("日期类型取值不合法，仅允许：0 / 1 / 2"),
            "{message}"
        );
        assert!(
            message.contains("标准工时必须大于 0 且不超过 1440 分钟"),
            "{message}"
        );

        let ok = UpsertCalendarReq {
            calendar_date: "2026-10-01".to_string(),
            is_workday: 0,
            holiday_type: 1,
            standard_minutes: 480,
            remark: "国庆节".to_string(),
        };
        assert!(validate_upsert_calendar(&ok).is_ok(), "合法日历行必须通过");
    }

    /// 班次工时必须与上下班窗口自洽：`work_minutes + rest_minutes` 不得超过窗口分钟数，
    /// 否则 `derive_work_minutes` 的逐日封顶值（= 班次 `work_minutes`）会大于真实窗口，
    /// 请假时长与加班校验一起畸变。
    #[test]
    fn shift_validation_rejects_work_minutes_exceeding_window() {
        // 09:00–18:00 = 540 分钟窗口，却声明 800 分钟工时
        let req = CreateShiftReq {
            work_minutes: 800,
            rest_minutes: 0,
            ..valid_shift_req()
        };
        let message = validate_create_shift(&req, STATUS_ALLOWED).unwrap_err();
        assert!(
            message.contains("应工作分钟数与休息分钟数之和不能超过班次窗口"),
            "{message}"
        );

        // 工时 + 休息超出窗口（480 + 120 > 540）
        let req = CreateShiftReq {
            work_minutes: 480,
            rest_minutes: 120,
            ..valid_shift_req()
        };
        let message = validate_create_shift(&req, STATUS_ALLOWED).unwrap_err();
        assert!(
            message.contains("应工作分钟数与休息分钟数之和不能超过班次窗口"),
            "{message}"
        );

        // 基线（480 + 60 = 540，正好等于窗口）必须仍然合法
        assert!(
            validate_create_shift(&valid_shift_req(), STATUS_ALLOWED).is_ok(),
            "工时 + 休息等于窗口必须通过"
        );
    }

    /// 跨天班的起止时间文案必须与规则一致：`end > start` 才是非法（`end == start` 是 24 小时窗口）。
    #[test]
    fn shift_cross_day_message_matches_rule() {
        let req = CreateShiftReq {
            start_time: "09:00:00".to_string(),
            end_time: "10:00:00".to_string(),
            cross_day: 1,
            ..valid_shift_req()
        };
        let message = validate_create_shift(&req, STATUS_ALLOWED).unwrap_err();
        assert!(
            message.contains("跨天班的下班时间不得晚于上班时间"),
            "{message}"
        );

        let same = CreateShiftReq {
            start_time: "09:00:00".to_string(),
            end_time: "09:00:00".to_string(),
            cross_day: 1,
            work_minutes: 1380,
            rest_minutes: 60,
            ..valid_shift_req()
        };
        assert!(
            validate_create_shift(&same, STATUS_ALLOWED).is_ok(),
            "跨天班 24 小时窗口（end == start）应通过：{}",
            validate_create_shift(&same, STATUS_ALLOWED).unwrap_err()
        );
    }

    /// 批量排班必须挡「人数 × 天数」总量：两个上限各自成立时乘积仍可到 36 万行，
    /// 单事务逐行读写会超时并整体回滚。
    #[test]
    fn batch_create_schedules_rejects_oversized_person_day_product() {
        let req = BatchCreateScheduleReq {
            employee_ids: (1..=1000).collect(),
            start_date: "2026-01-01".to_string(),
            end_date: "2026-12-31".to_string(),
            shift_id: 1,
            status: crate::modules::biz::hr::attendance::SCHEDULE_STATUS_NORMAL,
            remark: String::new(),
        };
        let message = validate_batch_create_schedules(&req).unwrap_err();
        assert!(message.contains("单次排班总行数不能超过"), "{message}");

        let ok = BatchCreateScheduleReq {
            employee_ids: (1..=10).collect(),
            start_date: "2026-01-01".to_string(),
            end_date: "2026-01-31".to_string(),
            shift_id: 1,
            status: crate::modules::biz::hr::attendance::SCHEDULE_STATUS_NORMAL,
            remark: String::new(),
        };
        assert!(
            validate_batch_create_schedules(&ok).is_ok(),
            "10 人 × 31 天应在上限内"
        );
    }
}
