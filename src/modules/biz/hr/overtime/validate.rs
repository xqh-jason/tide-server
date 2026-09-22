//! 加班请求体校验（纯函数，无第三方校验框架）。
//!
//! 结构同 approval / time_off 域：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req) -> Result<(), String>`，命中规则即**累积**中文错误消息，多条用「；」
//! 拼接后返回。handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! 值域来源唯一：`overtime_type` / `comp_mode` 是本域常量（`mod.rs`），不经字典；
//! 时间入参用 `utils::datetime::parse_datetime` 解析（格式错给中文提示，不做 400 反序列化错误）。
//!
//! 需要查库的规则（员工档案存在且未离职、应出勤日与加班类型是否匹配、同日区间是否重叠、
//! 与 `work_date` 是否同日）留在 service 层。

use crate::modules::biz::hr::overtime::dto::{CreateOvertimeReq, UpdateOvertimeReq};
use crate::modules::biz::hr::overtime::{COMP_MODES, OVERTIME_TYPES};
use crate::utils::datetime::parse_datetime;

/// 加班事由长度上限（对齐 `VARCHAR(255)`）。
const REASON_MAX: usize = 255;
/// 备注长度上限（对齐 `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;

/// 可选文本长度上限（空串放行，按 `char` 计，避免 UTF-8 边界误判）。
fn check_optional_len(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.chars().count() > max {
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

/// 必填时间：解析成功即通过；空串 / 格式错分别给中文提示。
fn check_required_datetime(raw: &str, label: &str, errors: &mut Vec<String>) {
    match parse_datetime(label, &Some(raw.to_string()), false) {
        Ok(Some(_)) => {}
        Ok(None) => errors.push(format!("{label}不能为空")),
        Err(err) => errors.push(err.to_string()),
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

/// 创建 / 更新加班单共用的字段视图（两个 DTO 除 `id` 外字段同名同型）。
struct OvertimeFields<'a> {
    employee_id: u64,
    work_date: &'a str,
    start_at: &'a str,
    end_at: &'a str,
    overtime_type: i8,
    comp_mode: i8,
    reason: &'a str,
    remark: &'a str,
}

/// 创建 / 更新加班单共用字段检查。
///
/// 规则（命中即累积，最后 `errors.join("；")`；文案是契约，测试断言子串）：
/// - `employee_id == 0` → `员工 ID 必须大于 0`；
/// - `work_date` 非 `yyyy-MM-dd` → `加班日期格式错误，应为 yyyy-MM-dd`；
/// - `start_at` / `end_at` 为空 → `加班开始时间不能为空` / `加班结束时间不能为空`；
///   格式错 → `utils::datetime::parse_datetime` 的中文提示（带字段名）；
/// - `start_at >= end_at` → `加班结束时间必须晚于开始时间`；
/// - `overtime_type` ∉ {1,2,3} → `加班类型取值不合法，仅允许：1 / 2 / 3`；
/// - `comp_mode` ∉ {1,2} → `补偿方式取值不合法，仅允许：1 / 2`；
/// - `reason` / `remark` 长度 > 255 → `…长度不能超过 255 个字符`；
/// - `attachment_id` 无值域约束（0 = 无附件）。
fn check_overtime_fields(fields: OvertimeFields<'_>) -> Vec<String> {
    let mut errors = Vec::new();

    if fields.employee_id == 0 {
        errors.push("员工 ID 必须大于 0".to_string());
    }
    if chrono::NaiveDate::parse_from_str(fields.work_date.trim(), "%Y-%m-%d").is_err() {
        errors.push("加班日期格式错误，应为 yyyy-MM-dd".to_string());
    }
    check_required_datetime(fields.start_at, "加班开始时间", &mut errors);
    check_required_datetime(fields.end_at, "加班结束时间", &mut errors);
    // 两个时间都可解析时才比大小，避免用「格式错但能解析成别的值」的结果做判断
    if let (Ok(Some(start)), Ok(Some(end))) = (
        parse_datetime("加班开始时间", &Some(fields.start_at.to_string()), false),
        parse_datetime("加班结束时间", &Some(fields.end_at.to_string()), false),
    ) && end <= start
    {
        errors.push("加班结束时间必须晚于开始时间".to_string());
    }

    check_int_in(
        fields.overtime_type,
        &OVERTIME_TYPES,
        "加班类型",
        &mut errors,
    );
    check_int_in(fields.comp_mode, &COMP_MODES, "补偿方式", &mut errors);
    check_optional_len(fields.reason, "加班事由", REASON_MAX, &mut errors);
    check_optional_len(fields.remark, "备注", REMARK_MAX, &mut errors);

    errors
}

/// 创建加班单校验（字段规则见 [`check_overtime_fields`]）。
pub fn validate_create_overtime(req: &CreateOvertimeReq) -> Result<(), String> {
    join_errors(check_overtime_fields(OvertimeFields {
        employee_id: req.employee_id,
        work_date: &req.work_date,
        start_at: &req.start_at,
        end_at: &req.end_at,
        overtime_type: req.overtime_type,
        comp_mode: req.comp_mode,
        reason: &req.reason,
        remark: &req.remark,
    }))
}

/// 更新加班单校验：字段规则同创建 + `id == 0 → 加班单 ID 必须大于 0`。
pub fn validate_update_overtime(req: &UpdateOvertimeReq) -> Result<(), String> {
    let mut errors = check_overtime_fields(OvertimeFields {
        employee_id: req.employee_id,
        work_date: &req.work_date,
        start_at: &req.start_at,
        end_at: &req.end_at,
        overtime_type: req.overtime_type,
        comp_mode: req.comp_mode,
        reason: &req.reason,
        remark: &req.remark,
    });
    if req.id == 0 {
        errors.push("加班单 ID 必须大于 0".to_string());
    }
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法基线：各用例只改被测字段，其余保持合法（避免副作用误报）。
    fn valid_create() -> CreateOvertimeReq {
        CreateOvertimeReq {
            employee_id: 1,
            work_date: "2026-09-19".to_string(),
            start_at: "2026-09-19 19:00:00".to_string(),
            end_at: "2026-09-19 21:30:00".to_string(),
            overtime_type: 1,
            comp_mode: 1,
            reason: "版本上线".to_string(),
            attachment_id: 0,
            remark: String::new(),
        }
    }

    #[test]
    fn valid_request_passes() {
        assert!(validate_create_overtime(&valid_create()).is_ok());
    }

    #[test]
    fn errors_accumulate_and_join_with_fullwidth_semicolon() {
        let req = CreateOvertimeReq {
            employee_id: 0,
            work_date: "2026/09/19".to_string(),
            start_at: "2026-09-19 21:00:00".to_string(),
            end_at: "2026-09-19 19:00:00".to_string(),
            overtime_type: 9,
            comp_mode: 0,
            ..valid_create()
        };
        let err = validate_create_overtime(&req).unwrap_err();
        assert!(err.contains("员工 ID 必须大于 0"), "应报员工 ID：{err}");
        assert!(
            err.contains("加班日期格式错误，应为 yyyy-MM-dd"),
            "应报日期格式：{err}"
        );
        assert!(
            err.contains("加班结束时间必须晚于开始时间"),
            "应报时间顺序：{err}"
        );
        assert!(
            err.contains("加班类型取值不合法，仅允许：1 / 2 / 3"),
            "应报类型值域：{err}"
        );
        assert!(
            err.contains("补偿方式取值不合法，仅允许：1 / 2"),
            "应报补偿方式值域：{err}"
        );
        assert!(err.contains("；"), "多条错误必须用中文分号拼接：{err}");
    }

    #[test]
    fn blank_datetime_reports_required_message() {
        let req = CreateOvertimeReq {
            start_at: "  ".to_string(),
            ..valid_create()
        };
        let err = validate_create_overtime(&req).unwrap_err();
        assert_eq!(err, "加班开始时间不能为空");
    }

    #[test]
    fn invalid_datetime_format_reports_parse_message_with_field_name() {
        let req = CreateOvertimeReq {
            end_at: "2026-09-19".to_string(),
            ..valid_create()
        };
        let err = validate_create_overtime(&req).unwrap_err();
        // 纯日期是合法入参（补 00:00:00），与 19:00 起始时间相比仍更早 → 报时间顺序
        assert!(
            err.contains("加班结束时间必须晚于开始时间"),
            "应报时间顺序：{err}"
        );

        let req = CreateOvertimeReq {
            end_at: "19:00".to_string(),
            ..valid_create()
        };
        let err = validate_create_overtime(&req).unwrap_err();
        assert!(err.contains("加班结束时间"), "提示应带字段名：{err}");
        assert!(err.contains("格式错误"), "格式错应给中文提示：{err}");
    }

    #[test]
    fn update_requires_positive_id() {
        let req = UpdateOvertimeReq {
            id: 0,
            employee_id: 1,
            work_date: "2026-09-19".to_string(),
            start_at: "2026-09-19 19:00:00".to_string(),
            end_at: "2026-09-19 21:30:00".to_string(),
            overtime_type: 2,
            comp_mode: 2,
            reason: String::new(),
            attachment_id: 0,
            remark: String::new(),
        };
        let err = validate_update_overtime(&req).unwrap_err();
        assert_eq!(err, "加班单 ID 必须大于 0");
    }
}
