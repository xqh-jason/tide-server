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

use crate::modules::biz::hr::employee::dto::{CreateEmployeeReq, UpdateEmployeeReq};

/// 创建员工档案校验：账号来源二选一 + 账号必填项 + 日期格式 + 字段长度 + 值域。
///
/// 长度上限（按 `char` 计数，对齐 `hr_employee` 列定义）：
/// `graduate_school` / `major` 128、`id_card` 32、`emergency_contact` 64、
/// `emergency_phone` 32、`bank_account` 64、`remark` 255。
// 骨架交接：`api.rs` 的 handler 实现后会调用本函数，届时删除本行。
#[allow(dead_code)]
pub fn validate_create_employee(
    req: &CreateEmployeeReq,
    employment_status_allowed: &[i8],
    education_allowed: &[i8],
) -> Result<(), String> {
    // 实现提示：
    // 1) 账号来源：`match (&req.user_id, &req.create_account)`
    //    - 都传 → 「userId 与 createAccount 只能二选一」
    //    - 都不传 → 「必须指定 userId 或 createAccount」
    //    - `Some(id)` 且 `*id == 0` → 「userId 必须大于 0」
    //    - `Some(acc)` → 账号必填项（trim 后非空）：「登录账号不能为空」「初始密码不能为空」
    //      「用户昵称不能为空」；username / nickname 长度上限 64；password 长度上限 128
    // 2) 三个日期字段（入职日期 / 转正日期 / 离职日期）：非空则必须能按 `%Y-%m-%d` 解析，
    //    否则 `「{label}格式应为 yyyy-MM-dd」`
    // 3) 长度：按 `char` 计数，超出报 `「{label}长度不能超过 {max} 个字符」`
    //    （helper 可仿 position 域 `check_len`；敏感字段空串合法 = 不设置）
    // 4) 值域：`check::check_status(req.employment_status, employment_status_allowed)`
    //    错误消息原样收下；`education` 不在 `education_allowed` 时报
    //    「学历取值不合法，仅允许：{choices}」（choices 用 ` / ` 拼接，同 check_status 风格）
    // 5) 错误累积进 `Vec<String>`，末了 `errors.join("；")`，空则 `Ok(())`
    let _ = (req, employment_status_allowed, education_allowed);
    Err("未实现：validate_create_employee".to_string())
}

/// 更新员工档案校验：同创建，但无账号字段，另加 id 必须大于 0。
///
/// 敏感字段（`id_card` / `bank_account`）**空串合法**：语义是「不修改」
/// （列表 / 详情回传掩码值，前端编辑表单不回填）。
// 骨架交接：`api.rs` 的 handler 实现后会调用本函数，届时删除本行。
#[allow(dead_code)]
pub fn validate_update_employee(
    req: &UpdateEmployeeReq,
    employment_status_allowed: &[i8],
    education_allowed: &[i8],
) -> Result<(), String> {
    // 实现提示：与 create 同一套规则，去掉账号二选一；`req.id == 0` 报
    // 「员工档案 ID 必须大于 0」；长度与日期规则复用同一批私有 helper
    let _ = (req, employment_status_allowed, education_allowed);
    Err("未实现：validate_update_employee".to_string())
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
