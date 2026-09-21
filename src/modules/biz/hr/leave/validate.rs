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
//!
//! **本模块当前是桩**：函数体一律返回 `Err("未实现：<函数名>")`，由作者按各函数的
//! `// 实现提示` 补齐；下方 5 条测试保持红灯即交接信号（原因恒为 `未实现：…`）。
//!
//! 三个函数上的 `#[cfg_attr(not(test), allow(dead_code))]` 是骨架期必需品：调用方 `api.rs`
//! 的函数体尚未接上，非测试构建下它们没有任何用户（test 构建下由下方用例覆盖）；
//! **`api.rs` 的函数体补齐时一并删除这三个属性**。

use crate::modules::biz::hr::leave::dto::{
    BatchCreateGrantReq, CreateLeaveTypeReq, UpdateLeaveTypeReq,
};

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
#[cfg_attr(not(test), allow(dead_code))]
pub fn validate_create_leave_type(
    req: &CreateLeaveTypeReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let _ = (req, status_allowed);
    Err("未实现：validate_create_leave_type".to_string())
}

/// 更新假期类型校验：规则同创建，另加主键检查。
///
/// 追加规则：`id` = 0 → `假期类型 ID 必须大于 0`；其余字段文案与
/// [`validate_create_leave_type`] 逐字一致。
//
// 实现提示：除 id 检查外与创建共用同一组 `check_*` 小函数；`UpdateLeaveTypeReq` 与
// `CreateLeaveTypeReq` 字段同名同型（多一个 `id`），拆一个 `&str`/`i8` 入参的共用内部函数即可。
// 骨架期：仅测试调用，实现函数体（api 层接线）后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub fn validate_update_leave_type(
    req: &UpdateLeaveTypeReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let _ = (req, status_allowed);
    Err("未实现：validate_update_leave_type".to_string())
}

/// 批量发放额度校验。
///
/// 规则（累积 + 「；」拼接）：
/// - 发放范围三选一：`all` = false 且 `employee_ids` 空且 `dept_id` 为 `None` →
///   `必须指定发放范围`；`all` = true 且同时给了 `employee_ids` / `dept_id` →
///   `all 与其他发放范围不能同时指定`；
/// - `leave_type_id` = 0 → `假期类型 ID 必须大于 0`；
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
#[cfg_attr(not(test), allow(dead_code))]
pub fn validate_batch_create_grant(req: &BatchCreateGrantReq) -> Result<(), String> {
    let _ = req;
    Err("未实现：validate_batch_create_grant".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `status` 允许值由平台字典提供（`check_status` 的唯一来源），测试用通用 0 / 1 档。
    const STATUS_ALLOWED: &[i8] = &[0, 1];

    /// 合法假期类型请求基线：各用例只改被测字段，其余保持合法（避免副作用误报）。
    fn valid_type_req() -> CreateLeaveTypeReq {
        CreateLeaveTypeReq {
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
            leave_type_id: 1,
            minutes: 2400,
            reason: "statutory".to_string(),
            period: "2026".to_string(),
            effective_at: "2026-01-01".to_string(),
            expire_at: Some("2026-12-31".to_string()),
            remark: String::new(),
        }
    }

    #[test]
    fn validate_create_leave_type_reports_all_errors_joined_by_semicolon() {
        let req = CreateLeaveTypeReq {
            type_code: String::new(),
            type_name: String::new(),
            unit: 9,
            min_unit_minutes: 0,
            ..valid_type_req()
        };

        let err = validate_create_leave_type(&req, STATUS_ALLOWED).unwrap_err();

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
    fn validate_create_leave_type_rejects_negative_min_unit_and_bad_code_charset() {
        let req = CreateLeaveTypeReq {
            type_code: "Annual-Leave".to_string(),
            min_unit_minutes: -30,
            ..valid_type_req()
        };

        let err = validate_create_leave_type(&req, STATUS_ALLOWED).unwrap_err();

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
            leave_type_id: 0,
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
    fn validate_update_leave_type_rejects_zero_id() {
        let req = UpdateLeaveTypeReq {
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

        let err = validate_update_leave_type(&req, STATUS_ALLOWED).unwrap_err();

        assert!(
            err.contains("假期类型 ID 必须大于 0"),
            "缺少 id 校验提示：{err}"
        );
    }
}
