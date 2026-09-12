//! 用户域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`），由调用方预取后作为
//! `status_allowed` 传入，通用判断见 `utils::check::check_status`。

use crate::modules::user::dto::{CreateUserReq, UpdateUserReq, UpdateUserStatusReq, UserDeptReq};
use crate::utils::check;

/// 手机号是否合法：11 位大陆手机号。
fn is_cn_phone(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 11
        && b[0] == b'1'
        && (b'3'..=b'9').contains(&b[1])
        && b.iter().all(u8::is_ascii_digit)
}

/// 邮箱宽松格式检查：允许「空串表示未设置」的项目约定，故未引入严格邮箱校验。
fn is_loose_email(s: &str) -> bool {
    if s.len() > 254 || s.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = s.split('@');
    let (Some(local), Some(domain)) = (parts.next(), parts.next()) else {
        return false;
    };
    if parts.next().is_some() || local.is_empty() || domain.is_empty() {
        return false;
    }
    domain.contains('.')
}

/// 用户名通用规则：2-20 个字符。
const USERNAME_LEN: (usize, usize) = (2, 20);

/// 校验「手机号/邮箱：空串=未设置；非空须合法」，供 create/update 共用。
fn check_phone_email(req: (&str, &str), errors: &mut Vec<String>) {
    let (phone, email) = req;
    if !phone.is_empty() && !is_cn_phone(phone) {
        errors.push("手机号格式不正确（应为 11 位大陆手机号）".to_string());
    }
    if email.chars().count() > 64 {
        errors.push("邮箱长度不能超过 64 个字符".to_string());
    } else if !email.is_empty() && !is_loose_email(email) {
        errors.push("邮箱格式不正确".to_string());
    }
}

fn check_username(username: &str, errors: &mut Vec<String>) {
    if !(USERNAME_LEN.0..=USERNAME_LEN.1).contains(&username.chars().count()) {
        errors.push("用户名长度须在 2-20 个字符之间".to_string());
    }
}

/// 工号：空串表示未设置（放行）；非空必须为 6 位 ASCII 数字，供 create/update 共用。
fn check_emp_no(emp_no: &str, errors: &mut Vec<String>) {
    if !emp_no.is_empty() && (emp_no.len() != 6 || !emp_no.bytes().all(|b| b.is_ascii_digit())) {
        errors.push("工号必须为 6 位数字（留空表示未设置）".to_string());
    }
}

fn check_nickname(nickname: &str, errors: &mut Vec<String>) {
    if nickname.chars().count() > 30 {
        errors.push("昵称长度不能超过 30 个字符".to_string());
    }
}

const MAX_USER_DEPTS: usize = 50;
/// 部门 ID：`dept_id` 为 `u64`，负数在 serde 反序列化即被拒；此处拦 `0`（0 = 系统/未设置，非合法部门）。
fn check_dept(depts: &[UserDeptReq], errors: &mut Vec<String>) {
    if depts.is_empty() {
        return;
    }

    if depts.iter().any(|d| d.dept_id == 0) {
        errors.push("部门 ID 不能为 0".to_string());
    }

    let is_primary_list = depts.iter().map(|d| d.is_primary).collect::<Vec<_>>();
    let is_primary_valid = is_primary_list.iter().all(|d| *d == 1 || *d == 0);
    if !is_primary_valid {
        errors.push("isPrimary 值必须为 1 或 0".to_string());
    }

    if depts.iter().filter(|d| d.is_primary == 1).count() != 1 {
        errors.push("挂载部门时必须有且仅有一个主部门".to_string());
    }

    let is_leader_list = depts.iter().map(|d| d.is_leader).collect::<Vec<_>>();
    let is_leader_valid = is_leader_list.iter().all(|d| *d == 1 || *d == 0);
    if !is_leader_valid {
        errors.push("isLeader 值必须为 1 或 0".to_string());
    }

    if depts.len() > MAX_USER_DEPTS {
        errors.push(format!("挂载部门数量不能超过 {} 个", MAX_USER_DEPTS));
    }

    let mut dept_id_list = depts.iter().map(|d| d.dept_id).collect::<Vec<_>>();
    let original_dept_id_count = dept_id_list.len();
    dept_id_list.sort();
    dept_id_list.dedup();
    let unique_dept_id_count = dept_id_list.len();
    if original_dept_id_count != unique_dept_id_count {
        errors.push("部门 ID 不能重复".to_string());
    }
}

/// 收集到的错误拼接为一条消息（按字段检查顺序，可读性优于只给首条）。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 创建用户请求校验。
pub fn validate_create_user(req: &CreateUserReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();

    // 密码：6-32 字符
    if !(6..=32).contains(&req.password.chars().count()) {
        errors.push("密码长度须在 6-32 个字符之间".to_string());
    }

    check_username(&req.username, &mut errors);
    check_nickname(&req.nickname, &mut errors);
    check_phone_email((&req.phone, &req.email), &mut errors);
    check_emp_no(&req.emp_no, &mut errors);
    check_dept(&req.depts, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();

    join_errors(errors)
}

/// 更新用户请求校验。
pub fn validate_update_user(req: &UpdateUserReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();

    // 目标用户 id
    if req.id == 0 {
        errors.push("用户 ID 必须大于 0".to_string());
    }
    check_username(&req.username, &mut errors);
    // 密码：空串表示不修改；非空须 6-32 字符
    if !req.password.is_empty() && !(6..=32).contains(&req.password.chars().count()) {
        errors.push("密码长度须在 6-32 个字符之间（留空表示不修改）".to_string());
    }
    // 工号：空串表示未设置；非空须 6 位 ASCII 数字
    check_emp_no(&req.emp_no, &mut errors);
    check_dept(&req.depts, &mut errors);
    check_nickname(&req.nickname, &mut errors);
    check_phone_email((&req.phone, &req.email), &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();

    join_errors(errors)
}

/// 更新用户状态请求校验。
pub fn validate_update_user_status(
    req: &UpdateUserStatusReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();

    if req.id == 0 {
        errors.push("用户 ID 必须大于 0".to_string());
    }
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();

    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreateUserReq {
        CreateUserReq {
            username: "user01".to_string(),
            password: "pass123".to_string(),
            emp_no: String::new(),
            nickname: "测试用户".to_string(),
            phone: "13800138000".to_string(),
            email: "user@example.com".to_string(),
            status: 1,
            role_ids: vec![],
            depts: vec![],
        }
    }

    fn update_req() -> UpdateUserReq {
        UpdateUserReq {
            id: 1,
            username: "user01".to_string(),
            password: String::new(),
            emp_no: "100001".to_string(),
            nickname: "测试用户".to_string(),
            phone: "13800138000".to_string(),
            email: "user@example.com".to_string(),
            status: 1,
            role_ids: vec![],
            depts: vec![],
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_user(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_username_too_short_reports_message() {
        let mut req = create_req();
        req.username = "u".to_string();
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("用户名长度须在 2-20 个字符之间"),
            "实际: {err}"
        );
    }

    #[test]
    fn create_invalid_phone_reports_message() {
        let mut req = create_req();
        req.phone = "12345".to_string();
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("手机号格式不正确"), "实际: {err}");
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn create_empty_email_or_phone_means_unset() {
        let mut req = create_req();
        req.phone.clear();
        req.email.clear();
        assert!(
            validate_create_user(&req, &[0, 1]).is_ok(),
            "空串表示未设置，不应报错"
        );
    }

    #[test]
    fn update_empty_password_passes() {
        assert!(
            validate_update_user(&update_req(), &[0, 1]).is_ok(),
            "密码留空表示不修改"
        );
    }

    #[test]
    fn update_invalid_emp_no_reports_message() {
        let mut req = update_req();
        req.emp_no = "abc123".to_string();
        let err = validate_update_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("工号必须为 6 位数字"), "实际: {err}");
    }

    #[test]
    fn update_empty_emp_no_passes() {
        let mut req = update_req();
        req.emp_no.clear();
        assert!(
            validate_update_user(&req, &[0, 1]).is_ok(),
            "工号留空表示未设置，不应报错"
        );
    }

    #[test]
    fn create_invalid_emp_no_reports_message() {
        let mut req = create_req();
        req.emp_no = "abc123".to_string();
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("工号必须为 6 位数字"), "实际: {err}");
    }

    #[test]
    fn create_short_emp_no_reports_message() {
        let mut req = create_req();
        req.emp_no = "123".to_string();
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("工号必须为 6 位数字"), "实际: {err}");
    }

    #[test]
    fn update_password_too_short_reports_message() {
        let mut req = update_req();
        req.password = "123".to_string();
        let err = validate_update_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("密码长度须在 6-32 个字符之间"), "实际: {err}");
    }

    #[test]
    fn create_dept_id_zero_reports_message() {
        let mut req = create_req();
        req.depts = vec![UserDeptReq {
            dept_id: 0,
            is_primary: 1,
            is_leader: 0,
        }];
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("部门 ID 不能为 0"), "实际: {err}");
    }

    #[test]
    fn create_valid_dept_passes() {
        let mut req = create_req();
        req.depts = vec![UserDeptReq {
            dept_id: 5,
            is_primary: 1,
            is_leader: 0,
        }];
        assert!(
            validate_create_user(&req, &[0, 1]).is_ok(),
            "合法部门 ID 不应报错"
        );
    }

    #[test]
    fn create_duplicate_dept_ids_reports_message() {
        let mut req = create_req();
        req.depts = vec![
            UserDeptReq {
                dept_id: 5,
                is_primary: 1,
                is_leader: 0,
            },
            UserDeptReq {
                dept_id: 5,
                is_primary: 0,
                is_leader: 1,
            },
        ];
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("部门 ID 不能重复"), "实际: {err}");
    }

    #[test]
    fn create_two_primary_depts_reports_message() {
        let mut req = create_req();
        req.depts = vec![
            UserDeptReq {
                dept_id: 5,
                is_primary: 1,
                is_leader: 0,
            },
            UserDeptReq {
                dept_id: 6,
                is_primary: 1,
                is_leader: 0,
            },
        ];
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("必须有且仅有一个主部门"), "实际: {err}");
    }

    #[test]
    fn create_depts_without_primary_reports_message() {
        let mut req = create_req();
        req.depts = vec![
            UserDeptReq {
                dept_id: 5,
                is_primary: 0,
                is_leader: 0,
            },
            UserDeptReq {
                dept_id: 6,
                is_primary: 0,
                is_leader: 1,
            },
        ];
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("必须有且仅有一个主部门"), "实际: {err}");
    }

    #[test]
    fn create_too_many_depts_reports_message() {
        let mut req = create_req();
        req.depts = (1u64..=51)
            .map(|i| UserDeptReq {
                dept_id: i,
                is_primary: if i == 1 { 1 } else { 0 },
                is_leader: 0,
            })
            .collect();
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("不能超过 50"), "实际: {err}");
    }

    #[test]
    fn create_invalid_primary_value_reports_message() {
        let mut req = create_req();
        req.depts = vec![UserDeptReq {
            dept_id: 5,
            is_primary: 2,
            is_leader: 0,
        }];
        let err = validate_create_user(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("必须为 1 或 0"), "实际: {err}");
    }

    #[test]
    fn create_multiple_leaders_passes() {
        let mut req = create_req();
        req.depts = vec![
            UserDeptReq {
                dept_id: 5,
                is_primary: 1,
                is_leader: 1,
            },
            UserDeptReq {
                dept_id: 6,
                is_primary: 0,
                is_leader: 1,
            },
        ];
        assert!(
            validate_create_user(&req, &[0, 1]).is_ok(),
            "允许多个部门负责人"
        );
    }
}
