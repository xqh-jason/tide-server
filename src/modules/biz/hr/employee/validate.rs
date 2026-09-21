//! 员工档案请求体校验（纯函数，无第三方校验框架）。
//!
//! 结构同 position 域：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, employment_status_allowed, education_allowed) -> Result<(), String>`，
//! 命中规则即累积中文错误消息，多条用「；」拼接后返回（可读性优于只回首条）。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! 两个值域都由数据字典提供（`dictionary::service::enabled_int_values`），
//! 由调用方预取后作为 `*_allowed` 传入，**不硬编码**：
//! - `employment_status`：字典 `employmentStatus`，通用判断 `utils::check::check_status`
//!   （文案「状态取值不合法，仅允许：…」）；
//! - `education`：字典 `education`，`i8` 直接 `contains` 判断，文案同款
//!   「学历取值不合法，仅允许：…」。
//!
//! 需要查库的规则（`user_id` 查重、账号存在性、档案存在性）留在 service 层。

use crate::modules::biz::hr::employee::dto::{
    CreateAccountReq, CreateEmployeeReq, UpdateEmployeeReq,
};
use crate::utils::check;

/// 毕业院校 / 专业长度上限（对齐 `VARCHAR(128)`）。
const GRADUATE_SCHOOL_MAX: usize = 128;
const MAJOR_MAX: usize = 128;
/// 身份证号长度上限（对齐 `VARCHAR(32)`）。
const ID_CARD_MAX: usize = 32;
/// 紧急联系人 / 紧急电话长度上限。
const EMERGENCY_CONTACT_MAX: usize = 64;
const EMERGENCY_PHONE_MAX: usize = 32;
/// 工资卡号长度上限（对齐 `VARCHAR(64)`）。
const BANK_ACCOUNT_MAX: usize = 64;
/// 备注长度上限（对齐 `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;
/// 账号字段长度上限（对齐 `sys_user` 列宽）。
const ACCOUNT_USERNAME_MAX: usize = 64;
const ACCOUNT_PASSWORD_MAX: usize = 128;
const ACCOUNT_NICKNAME_MAX: usize = 64;

/// 检查必填文本字段：trim 后非空，且不超过列长度。
fn check_required(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.trim().is_empty() {
        errors.push(format!("{label}不能为空"));
    } else if s.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 检查可选文本字段：仅限长度（空串合法，表示未设置）。
fn check_len(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 检查可选日期字段：非空必须能按 `yyyy-MM-dd` 解析。
fn check_date(v: &Option<String>, label: &str, errors: &mut Vec<String>) {
    let Some(raw) = v.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").is_err() {
        errors.push(format!("{label}格式应为 yyyy-MM-dd"));
    }
}

/// 检查学历值域（字典 `education`）：文案风格与 `utils::check::check_status` 一致。
fn check_education(value: i8, allowed: &[i8], errors: &mut Vec<String>) {
    if allowed.contains(&value) {
        return;
    }
    if allowed.is_empty() {
        errors.push("学历取值不合法".to_string());
        return;
    }
    let choices = allowed
        .iter()
        .map(i8::to_string)
        .collect::<Vec<_>>()
        .join(" / ");
    errors.push(format!("学历取值不合法，仅允许：{choices}"));
}

/// 账号来源二选一 + 账号必填项（创建专用）。
fn check_account_source(
    user_id: &Option<u64>,
    create_account: &Option<CreateAccountReq>,
    errors: &mut Vec<String>,
) {
    match (user_id, create_account) {
        (Some(_), Some(_)) => errors.push("userId 与 createAccount 只能二选一".to_string()),
        (None, None) => errors.push("必须指定 userId 或 createAccount".to_string()),
        (Some(0), None) => errors.push("userId 必须大于 0".to_string()),
        (Some(_), None) => {}
        (None, Some(acc)) => {
            check_required(&acc.username, "登录账号", ACCOUNT_USERNAME_MAX, errors);
            check_required(&acc.password, "初始密码", ACCOUNT_PASSWORD_MAX, errors);
            check_required(&acc.nickname, "用户昵称", ACCOUNT_NICKNAME_MAX, errors);
        }
    }
}

/// 值域检查（创建 / 更新共用）：在职状态走通用 `check_status`，学历同款风格。
fn check_value_domains(
    employment_status: i8,
    education: i8,
    employment_status_allowed: &[i8],
    education_allowed: &[i8],
    errors: &mut Vec<String>,
) {
    check::check_status(employment_status, employment_status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    check_education(education, education_allowed, errors);
}

/// 收集到的错误拼接为一条消息（按字段检查顺序，可读性优于只给首条）。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 创建员工档案校验：账号来源二选一 + 账号必填项 + 日期格式 + 字段长度 + 值域。
///
/// 长度上限（按 `char` 计数，对齐 `hr_employee` 列定义）：
/// `graduate_school` / `major` 128、`id_card` 32、`emergency_contact` 64、
/// `emergency_phone` 32、`bank_account` 64、`remark` 255。
pub fn validate_create_employee(
    req: &CreateEmployeeReq,
    employment_status_allowed: &[i8],
    education_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    check_account_source(&req.user_id, &req.create_account, &mut errors);
    check_date(&req.hire_date, "入职日期", &mut errors);
    check_date(&req.regular_date, "转正日期", &mut errors);
    check_date(&req.leave_date, "离职日期", &mut errors);
    check_len(
        &req.graduate_school,
        "毕业院校",
        GRADUATE_SCHOOL_MAX,
        &mut errors,
    );
    check_len(&req.major, "专业", MAJOR_MAX, &mut errors);
    check_len(&req.id_card, "身份证号", ID_CARD_MAX, &mut errors);
    check_len(
        &req.emergency_contact,
        "紧急联系人",
        EMERGENCY_CONTACT_MAX,
        &mut errors,
    );
    check_len(
        &req.emergency_phone,
        "紧急电话",
        EMERGENCY_PHONE_MAX,
        &mut errors,
    );
    check_len(&req.bank_account, "工资卡号", BANK_ACCOUNT_MAX, &mut errors);
    check_len(&req.remark, "备注", REMARK_MAX, &mut errors);
    check_value_domains(
        req.employment_status,
        req.education,
        employment_status_allowed,
        education_allowed,
        &mut errors,
    );
    join_errors(errors)
}

/// 更新员工档案校验：同创建，但无账号字段，另加 id 必须大于 0。
///
/// 敏感字段（`id_card` / `bank_account`）**空串合法**：语义是「不修改」
/// （列表 / 详情回传掩码值，前端编辑表单不回填）。
pub fn validate_update_employee(
    req: &UpdateEmployeeReq,
    employment_status_allowed: &[i8],
    education_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("员工档案 ID 必须大于 0".to_string());
    }
    check_date(&req.hire_date, "入职日期", &mut errors);
    check_date(&req.regular_date, "转正日期", &mut errors);
    check_date(&req.leave_date, "离职日期", &mut errors);
    check_len(
        &req.graduate_school,
        "毕业院校",
        GRADUATE_SCHOOL_MAX,
        &mut errors,
    );
    check_len(&req.major, "专业", MAJOR_MAX, &mut errors);
    check_len(&req.id_card, "身份证号", ID_CARD_MAX, &mut errors);
    check_len(
        &req.emergency_contact,
        "紧急联系人",
        EMERGENCY_CONTACT_MAX,
        &mut errors,
    );
    check_len(
        &req.emergency_phone,
        "紧急电话",
        EMERGENCY_PHONE_MAX,
        &mut errors,
    );
    check_len(&req.bank_account, "工资卡号", BANK_ACCOUNT_MAX, &mut errors);
    check_len(&req.remark, "备注", REMARK_MAX, &mut errors);
    check_value_domains(
        req.employment_status,
        req.education,
        employment_status_allowed,
        education_allowed,
        &mut errors,
    );
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::biz::hr::employee::dto::CreateAccountReq;

    /// 字典允许值：`employmentStatus` = 1 在职 / 2 试用 / 3 离职；`education` = 0..=5。
    const STATUS_ALLOWED: &[i8] = &[1, 2, 3];
    const EDUCATION_ALLOWED: &[i8] = &[0, 1, 2, 3, 4, 5];

    fn req() -> CreateEmployeeReq {
        CreateEmployeeReq {
            user_id: Some(1),
            create_account: None,
            hire_date: Some("2026-01-01".to_string()),
            regular_date: None,
            leave_date: None,
            employment_status: 1,
            education: 0,
            graduate_school: String::new(),
            major: String::new(),
            id_card: String::new(),
            emergency_contact: String::new(),
            emergency_phone: String::new(),
            bank_account: String::new(),
            remark: String::new(),
        }
    }

    fn account() -> CreateAccountReq {
        CreateAccountReq {
            username: "zhangsan".to_string(),
            password: "Passw0rd!23".to_string(),
            nickname: "张三".to_string(),
            emp_no: String::new(),
            phone: String::new(),
            email: String::new(),
            role_ids: vec![],
        }
    }

    fn update_req() -> UpdateEmployeeReq {
        UpdateEmployeeReq {
            id: 1,
            hire_date: None,
            regular_date: None,
            leave_date: None,
            employment_status: 1,
            education: 3,
            graduate_school: String::new(),
            major: String::new(),
            id_card: String::new(),
            emergency_contact: String::new(),
            emergency_phone: String::new(),
            bank_account: String::new(),
            remark: String::new(),
        }
    }

    #[test]
    fn valid_request_passes() {
        assert!(validate_create_employee(&req(), STATUS_ALLOWED, EDUCATION_ALLOWED).is_ok());
        assert!(validate_update_employee(&update_req(), STATUS_ALLOWED, EDUCATION_ALLOWED).is_ok());
    }

    #[test]
    fn requires_exactly_one_account_source() {
        let mut neither = req();
        neither.user_id = None;
        let err =
            validate_create_employee(&neither, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("userId 或 createAccount"), "实际：{err}");

        let mut both = req();
        both.create_account = Some(account());
        let err = validate_create_employee(&both, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("只能二选一"), "实际：{err}");

        let mut zero = req();
        zero.user_id = Some(0);
        let err = validate_create_employee(&zero, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("userId 必须大于 0"), "实际：{err}");
    }

    #[test]
    fn blank_account_fields_are_rejected() {
        let mut r = req();
        r.user_id = None;
        let mut acc = account();
        acc.username = "  ".to_string();
        acc.password = String::new();
        acc.nickname = String::new();
        r.create_account = Some(acc);
        let err = validate_create_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("登录账号不能为空"), "实际：{err}");
        assert!(err.contains("初始密码不能为空"), "实际：{err}");
        assert!(err.contains("用户昵称不能为空"), "实际：{err}");
    }

    #[test]
    fn invalid_employment_status_is_rejected() {
        let mut r = req();
        r.employment_status = 9;
        let err = validate_create_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际：{err}");
    }

    #[test]
    fn invalid_education_is_rejected() {
        let mut r = req();
        r.education = 9;
        let err = validate_create_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("学历取值不合法"), "实际：{err}");
    }

    #[test]
    fn malformed_date_is_rejected() {
        let mut r = req();
        r.hire_date = Some("2026/01/01".to_string());
        let err = validate_create_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("入职日期"), "实际：{err}");

        let mut r = req();
        r.regular_date = Some("2026-13-01".to_string());
        let err = validate_create_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("转正日期"), "实际：{err}");
    }

    #[test]
    fn overlong_fields_are_rejected() {
        let mut r = req();
        r.remark = "备".repeat(256);
        r.id_card = "1".repeat(33);
        r.emergency_phone = "1".repeat(33);
        let err = validate_create_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("备注长度不能超过 255"), "实际：{err}");
        assert!(err.contains("身份证号长度不能超过 32"), "实际：{err}");
        assert!(err.contains("紧急电话长度不能超过 32"), "实际：{err}");
    }

    #[test]
    fn update_requires_positive_id() {
        let mut r = update_req();
        r.id = 0;
        let err = validate_update_employee(&r, STATUS_ALLOWED, EDUCATION_ALLOWED).unwrap_err();
        assert!(err.contains("员工档案 ID 必须大于 0"), "实际：{err}");
    }
}
