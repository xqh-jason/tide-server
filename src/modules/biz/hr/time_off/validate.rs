//! 假期额度请求体校验（纯函数，无第三方校验框架）。
//!
//! 结构同 employee / position 域：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, …) -> Result<(), String>`，命中规则即累积中文错误消息，多条用「；」
//! 拼接后返回（可读性优于只回首条）。handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! `status` 的允许值唯一来源是平台字典（`dictionary::service::enabled_int_values`），由调用方
//! 预取后作为 `status_allowed` 传入，**不硬编码**；文案直接用 `utils::check::check_status`
//! 的返回（「状态取值不合法，仅允许：…」），不自己造句。
//!
//! 需要查库的规则（`type_code` 查重、假期类型存在性）留在 service 层。

use crate::modules::biz::hr::time_off::dto::{
    BatchCreateGrantReq, CreateTimeOffTypeReq, UpdateTimeOffTypeReq,
};
use crate::utils::check;

/// 类型编码长度上限（对齐 `VARCHAR(32)`）。
const TYPE_CODE_MAX: usize = 32;
/// 类型名称长度上限（对齐 `VARCHAR(64)`）。
const TYPE_NAME_MAX: usize = 64;
/// 备注长度上限（对齐 `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;
/// 单次批量发放人数上限。
const MAX_BATCH_EMPLOYEES: usize = 1000;

/// 创建 / 更新共用的字段视图（两个 DTO 除 `id` 外字段同名同型，抽视图避免复制粘贴）。
struct TypeFields<'a> {
    type_code: &'a str,
    type_name: &'a str,
    unit: i8,
    balance_mode: i8,
    min_unit_minutes: i32,
    require_attachment: i8,
    allow_negative: i8,
    pay_ratio: i32,
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
    let allowed_ok = allowed.contains(&value);
    if allowed_ok {
        return;
    }
    let choices = allowed
        .iter()
        .map(i8::to_string)
        .collect::<Vec<_>>()
        .join(" / ");
    errors.push(format!("{label}取值不合法，仅允许：{choices}"));
}

/// 类型编码字符集：只允许小写字母 / 数字 / 下划线。
fn check_type_code_charset(code: &str, errors: &mut Vec<String>) {
    let code = code.trim();
    if code.is_empty() {
        return;
    }
    let ok = code
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        errors.push("类型编码只能包含小写字母、数字与下划线".to_string());
    }
}

/// 创建 / 更新共用字段检查。
fn check_type_fields(fields: TypeFields<'_>, status_allowed: &[i8]) -> Vec<String> {
    let mut errors = Vec::new();

    check_required(fields.type_code, "类型编码", TYPE_CODE_MAX, &mut errors);
    check_type_code_charset(fields.type_code, &mut errors);
    check_required(fields.type_name, "类型名称", TYPE_NAME_MAX, &mut errors);
    check_int_in(fields.unit, &[1, 2], "计量单位", &mut errors);
    check_int_in(fields.balance_mode, &[0, 1], "额度模式", &mut errors);
    if fields.min_unit_minutes <= 0 {
        errors.push("最小请假单位必须大于 0".to_string());
    }
    check_int_in(
        fields.require_attachment,
        &[0, 1],
        "是否必须上传附件",
        &mut errors,
    );
    check_int_in(fields.allow_negative, &[0, 1], "是否允许超额", &mut errors);
    if fields.pay_ratio < 0 {
        errors.push("计薪比例不能小于 0".to_string());
    }
    if fields.remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }
    // 状态值域的唯一来源是平台字典，文案用 check_status 的返回，不自己造句
    check::check_status(fields.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();

    errors
}

/// 解析 `yyyy-MM-dd`；空串或格式错都记一条 `{label}格式应为 yyyy-MM-dd`。
fn parse_date(raw: &str, label: &str, errors: &mut Vec<String>) -> Option<chrono::NaiveDate> {
    match chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d") {
        Ok(date) => Some(date),
        Err(_) => {
            errors.push(format!("{label}格式应为 yyyy-MM-dd"));
            None
        }
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

/// 创建假期类型校验。
///
/// 规则（命中即累积进 `Vec<String>`，最后 `errors.join("；")`；下面文案是契约，测试断言子串）：
/// - `type_code`：trim 后非空 → `类型编码不能为空`；长度 > 32（按 `char`）→
///   `类型编码长度不能超过 32 个字符`；仅允许小写字母 / 数字 / 下划线 →
///   `类型编码只能包含小写字母、数字与下划线`；
/// - `type_name`：trim 后非空 → `类型名称不能为空`；长度 > 64 → `类型名称长度不能超过 64 个字符`；
/// - `unit` ∉ {1, 2} → `计量单位取值不合法，仅允许：1 / 2`；
/// - `balance_mode` ∉ {0, 1} → `额度模式取值不合法，仅允许：0 / 1`；
/// - `min_unit_minutes` ≤ 0 → `最小请假单位必须大于 0`；
/// - `require_attachment` / `allow_negative` ∉ {0, 1} →
///   `是否必须上传附件取值不合法，仅允许：0 / 1` / `是否允许超额取值不合法，仅允许：0 / 1`；
/// - `pay_ratio` < 0 → `计薪比例不能小于 0`；
/// - `remark` 长度 > 255 → `备注长度不能超过 255 个字符`；
/// - `status`：`check::check_status(req.status, status_allowed)` 的 `Err` 原样入列。
//
// 实现提示：照 `modules::biz::hr::employee::validate::validate_create_employee` 的形状写——
// 一组 `check_*` 私有小函数（必填 / 长度 / 值域，值域文案 `仅允许：1 / 2` 形式）+ `join_errors`；
// `status` 走 `crate::utils::check::check_status(req.status, status_allowed)
// .map_err(|e| errors.push(e)).ok();`（返回 `Result<(), String>`，非 `AppError`）。
// 骨架期：仅测试调用，实现函数体（api 层接线）后删除本属性
pub fn validate_create_time_off_type(
    req: &CreateTimeOffTypeReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let errors = check_type_fields(
        TypeFields {
            type_code: &req.type_code,
            type_name: &req.type_name,
            unit: req.unit,
            balance_mode: req.balance_mode,
            min_unit_minutes: req.min_unit_minutes,
            require_attachment: req.require_attachment,
            allow_negative: req.allow_negative,
            pay_ratio: req.pay_ratio,
            status: req.status,
            remark: &req.remark,
        },
        status_allowed,
    );
    join_errors(errors)
}

/// 更新假期类型校验：规则同创建，另加主键检查。
///
/// 追加规则：`id` = 0 → `假期类型 ID 必须大于 0`；其余字段文案与
/// [`validate_create_time_off_type`] 逐字一致。
//
// 实现提示：除 id 检查外与创建共用同一组 `check_*` 小函数；`UpdateTimeOffTypeReq` 与
// `CreateTimeOffTypeReq` 字段同名同型（多一个 `id`），拆一个 `&str`/`i8` 入参的共用内部函数即可。
// 骨架期：仅测试调用，实现函数体（api 层接线）后删除本属性
pub fn validate_update_time_off_type(
    req: &UpdateTimeOffTypeReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("假期类型 ID 必须大于 0".to_string());
    }
    errors.extend(check_type_fields(
        TypeFields {
            type_code: &req.type_code,
            type_name: &req.type_name,
            unit: req.unit,
            balance_mode: req.balance_mode,
            min_unit_minutes: req.min_unit_minutes,
            require_attachment: req.require_attachment,
            allow_negative: req.allow_negative,
            pay_ratio: req.pay_ratio,
            status: req.status,
            remark: &req.remark,
        },
        status_allowed,
    ));
    join_errors(errors)
}

/// 批量发放额度校验。
///
/// 规则（累积 + 「；」拼接）：
/// - 发放范围三选一：`all` = false 且 `employee_ids` 空且 `dept_id` 为 `None` →
///   `必须指定发放范围`；`all` = true 且同时给了 `employee_ids` / `dept_id` →
///   `all 与其他发放范围不能同时指定`；
/// - `time_off_type_id` = 0 → `假期类型 ID 必须大于 0`；
/// - `minutes` ≤ 0 → `发放时长必须大于 0`（字段是分钟数，文案用「时长」不用「天数」）；
/// - `reason` trim 后为空 → `发放依据不能为空`；`period` trim 后为空 → `归属周期不能为空`；
/// - `employee_ids` 去重后长度 > 1000 → `单次发放人数不能超过 1000`；
/// - `effective_at` 非 `yyyy-MM-dd` → `生效日期格式应为 yyyy-MM-dd`；
///   `expire_at` 非空且非 `yyyy-MM-dd` → `失效日期格式应为 yyyy-MM-dd`；
///   两者都能解析且 `expire_at < effective_at` → `失效日期不能早于生效日期`。
//
// 实现提示：日期解析用 `chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")`（同 employee 域）；
// 员工 ID 去重计数用 `crate::utils::check` 或就地 `HashSet`（`duplicate_ids` 是查重、不是去重）。
// 骨架期：仅测试调用，实现函数体（api 层接线）后删除本属性
pub fn validate_batch_create_grant(req: &BatchCreateGrantReq) -> Result<(), String> {
    let mut errors = Vec::new();

    // 发放范围三选一（互斥而非覆盖：`all` 与显式范围同时给出是语义冲突）
    let has_explicit_scope = !req.employee_ids.is_empty() || req.dept_id.is_some();
    if req.all && has_explicit_scope {
        errors.push("all 与其他发放范围不能同时指定".to_string());
    } else if !req.all && !has_explicit_scope {
        errors.push("必须指定发放范围".to_string());
    }

    if req.time_off_type_id == 0 {
        errors.push("假期类型 ID 必须大于 0".to_string());
    }
    if req.minutes <= 0 {
        errors.push("发放时长必须大于 0".to_string());
    }
    if req.reason.trim().is_empty() {
        errors.push("发放依据不能为空".to_string());
    }
    if req.period.trim().is_empty() {
        errors.push("归属周期不能为空".to_string());
    }

    let unique_count = crate::utils::user_ref::dedup_ids(req.employee_ids.clone()).len();
    if unique_count > MAX_BATCH_EMPLOYEES {
        errors.push(format!("单次发放人数不能超过 {MAX_BATCH_EMPLOYEES}"));
    }

    let effective_at = parse_date(&req.effective_at, "生效日期", &mut errors);
    let expire_at = req
        .expire_at
        .as_deref()
        .map(|raw| parse_date(raw, "失效日期", &mut errors))
        .unwrap_or(None);
    if let (Some(effective_at), Some(expire_at)) = (effective_at, expire_at)
        && expire_at < effective_at
    {
        errors.push("失效日期不能早于生效日期".to_string());
    }

    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `status` 允许值由平台字典提供（`check_status` 的唯一来源），测试用通用 0 / 1 档。
    const STATUS_ALLOWED: &[i8] = &[0, 1];

    /// 合法假期类型请求基线：各用例只改被测字段，其余保持合法（避免副作用误报）。
    fn valid_type_req() -> CreateTimeOffTypeReq {
        CreateTimeOffTypeReq {
            type_code: "annual".to_string(),
            type_name: "年假".to_string(),
            unit: 1,
            balance_mode: 1,
            min_unit_minutes: 240,
            require_attachment: 0,
            allow_negative: 0,
            pay_ratio: 1000,
            status: 1,
            remark: String::new(),
        }
    }

    /// 合法批量发放请求基线（范围 = 指定员工二人）。
    fn valid_grant_req() -> BatchCreateGrantReq {
        BatchCreateGrantReq {
            employee_ids: vec![1, 2],
            dept_id: None,
            all: false,
            time_off_type_id: 1,
            minutes: 2400,
            reason: "statutory".to_string(),
            period: "2026".to_string(),
            effective_at: "2026-01-01".to_string(),
            expire_at: Some("2026-12-31".to_string()),
            remark: String::new(),
        }
    }

    #[test]
    fn validate_create_time_off_type_reports_all_errors_joined_by_semicolon() {
        let req = CreateTimeOffTypeReq {
            type_code: String::new(),
            type_name: String::new(),
            unit: 9,
            min_unit_minutes: 0,
            ..valid_type_req()
        };

        let err = validate_create_time_off_type(&req, STATUS_ALLOWED).unwrap_err();

        assert!(err.contains("类型编码不能为空"), "缺少编码提示：{err}");
        assert!(err.contains("类型名称不能为空"), "缺少名称提示：{err}");
        assert!(
            err.contains("计量单位取值不合法，仅允许：1 / 2"),
            "缺少单位提示：{err}"
        );
        assert!(
            err.contains("最小请假单位必须大于 0"),
            "缺少最小单位提示：{err}"
        );
        assert!(err.contains("；"), "多条错误必须以中文分号连接");
    }

    #[test]
    fn validate_create_time_off_type_rejects_negative_min_unit_and_bad_code_charset() {
        let req = CreateTimeOffTypeReq {
            type_code: "Annual-Leave".to_string(),
            min_unit_minutes: -30,
            ..valid_type_req()
        };

        let err = validate_create_time_off_type(&req, STATUS_ALLOWED).unwrap_err();

        assert!(
            err.contains("类型编码只能包含小写字母、数字与下划线"),
            "编码字符集未拦住：{err}"
        );
        assert!(
            err.contains("最小请假单位必须大于 0"),
            "负数最小单位未拦住：{err}"
        );
    }

    #[test]
    fn validate_batch_create_grant_requires_scope_and_target() {
        // 范围三选一全空 + 假别缺省
        let req = BatchCreateGrantReq {
            employee_ids: Vec::new(),
            dept_id: None,
            all: false,
            time_off_type_id: 0,
            ..valid_grant_req()
        };
        let err = validate_batch_create_grant(&req).unwrap_err();
        assert!(err.contains("必须指定发放范围"), "缺少范围提示：{err}");
        assert!(
            err.contains("假期类型 ID 必须大于 0"),
            "缺少假别提示：{err}"
        );

        // all 与显式范围同时出现属于语义冲突，不能静默以 all 为准
        let conflicting = BatchCreateGrantReq {
            all: true,
            dept_id: Some(1),
            ..valid_grant_req()
        };
        let err = validate_batch_create_grant(&conflicting).unwrap_err();
        assert!(
            err.contains("all 与其他发放范围不能同时指定"),
            "未拦住范围冲突：{err}"
        );
    }

    #[test]
    fn validate_batch_create_grant_rejects_zero_or_negative_minutes() {
        for minutes in [0, -1] {
            let req = BatchCreateGrantReq {
                minutes,
                ..valid_grant_req()
            };

            let err = validate_batch_create_grant(&req).unwrap_err();

            assert!(
                err.contains("发放时长必须大于 0"),
                "minutes={minutes} 未拦住：{err}"
            );
        }
    }

    #[test]
    fn validate_update_time_off_type_rejects_zero_id() {
        let req = UpdateTimeOffTypeReq {
            id: 0,
            type_code: "annual".to_string(),
            type_name: "年假".to_string(),
            unit: 1,
            balance_mode: 1,
            min_unit_minutes: 240,
            require_attachment: 0,
            allow_negative: 0,
            pay_ratio: 1000,
            status: 1,
            remark: String::new(),
        };

        let err = validate_update_time_off_type(&req, STATUS_ALLOWED).unwrap_err();

        assert!(
            err.contains("假期类型 ID 必须大于 0"),
            "缺少 id 校验提示：{err}"
        );
    }
}
