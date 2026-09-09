//! 角色域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`），由调用方预取后作为
//! `status_allowed` 传入，通用判断见 `utils::check::check_status`。
//! 需要查库的规则（键/名查重、存在性、内置超管冻结）留在 service 层。

use crate::modules::permission::SUPER_ROLE_KEY;
use crate::modules::role::dto::{CreateRoleReq, UpdateRoleReq, UpdateRoleStatusReq};
use crate::utils::check;

/// 角色名称 / 角色键最大长度（对齐 `sys_role` 列定义 `VARCHAR(50)`）。
const NAME_MAX: usize = 50;
/// 备注最大长度（对齐 `sys_role.remark` `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;

/// 检查角色名称 / 角色键：trim 后非空，且不超过列长度。
fn check_required_name(name: &str, label: &str, errors: &mut Vec<String>) {
    if name.trim().is_empty() {
        errors.push(format!("{label}不能为空"));
    } else if name.chars().count() > NAME_MAX {
        errors.push(format!("{label}长度不能超过 {NAME_MAX} 个字符"));
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

/// 角色写请求通用字段校验（create/update 共用，不含 id）。
fn check_common_fields(req: (&str, &str, &str, i8, &[i8]), errors: &mut Vec<String>) {
    let (role_name, role_key, remark, status, status_allowed) = req;
    check_required_name(role_name, "角色名称", errors);
    check_required_name(role_key, "角色键", errors);
    // 保留字：任何角色（含新建）不得使用内置超管键。
    if role_key == SUPER_ROLE_KEY {
        errors.push("角色键 super 为系统保留字，不允许使用".to_string());
    }
    if remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }
    check::check_status(status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
}

/// 创建角色请求校验。
pub fn validate_create_role(req: &CreateRoleReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    check_common_fields(
        (
            &req.role_name,
            &req.role_key,
            &req.remark,
            req.status,
            status_allowed,
        ),
        &mut errors,
    );
    join_errors(errors)
}

/// 更新角色请求校验。
pub fn validate_update_role(req: &UpdateRoleReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("角色 ID 必须大于 0".to_string());
    }
    check_common_fields(
        (
            &req.role_name,
            &req.role_key,
            &req.remark,
            req.status,
            status_allowed,
        ),
        &mut errors,
    );
    join_errors(errors)
}

/// 更新角色状态请求校验。
pub fn validate_update_role_status(
    req: &UpdateRoleStatusReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("角色 ID 必须大于 0".to_string());
    }
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreateRoleReq {
        CreateRoleReq {
            role_name: "测试角色".to_string(),
            role_key: "test_role".to_string(),
            sort: 0,
            status: 1,
            remark: "validate 层测试".to_string(),
            menu_ids: vec![],
            api_ids: vec![],
        }
    }

    fn update_req() -> UpdateRoleReq {
        UpdateRoleReq {
            id: 1,
            role_name: "测试角色".to_string(),
            role_key: "test_role".to_string(),
            sort: 0,
            status: 1,
            remark: "validate 层测试".to_string(),
            menu_ids: vec![],
            api_ids: vec![],
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_role(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_empty_role_name_reports_message() {
        let mut req = create_req();
        req.role_name = "  ".to_string();
        let err = validate_create_role(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("角色名称不能为空"), "实际: {err}");
    }

    #[test]
    fn create_overlong_role_key_reports_message() {
        let mut req = create_req();
        req.role_key = "x".repeat(51);
        let err = validate_create_role(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("角色键长度不能超过 50 个字符"), "实际: {err}");
    }

    #[test]
    fn create_super_role_key_reports_reserved_word() {
        let mut req = create_req();
        req.role_key = SUPER_ROLE_KEY.to_string();
        let err = validate_create_role(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("super 为系统保留字"), "实际: {err}");
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_role(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_zero_id_reports_message() {
        let mut req = update_req();
        req.id = 0;
        let err = validate_update_role(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("角色 ID 必须大于 0"), "实际: {err}");
    }

    #[test]
    fn update_status_invalid_reports_message() {
        let mut req = update_req();
        req.status = 2;
        let err = validate_update_role(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_role_status_zero_id_reports_message() {
        let req = UpdateRoleStatusReq { id: 0, status: 1 };
        let err = validate_update_role_status(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("角色 ID 必须大于 0"), "实际: {err}");
    }
}
