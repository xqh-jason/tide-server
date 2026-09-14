//! 部门域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`），由调用方预取后作为 `status_allowed`
//! 传入，通用判断见 `utils::check::check_status`。
//! 需要查库的规则（父存在性、防环、同父同名）留在 service 层。

use crate::modules::system::dept::dto::{CreateDeptReq, UpdateDeptReq};
use crate::utils::check;

/// 部门名称最大长度（对齐 `sys_dept.dept_name` `VARCHAR(64)`）。
const NAME_MAX: usize = 64;
/// 备注最大长度（对齐 `sys_dept.remark` `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;

/// create / update 共用的字段校验（不含 id）。
fn check_common_fields(req: (&str, &str, i8, &[i8]), errors: &mut Vec<String>) {
    let (dept_name, remark, status, status_allowed) = req;
    if dept_name.trim().is_empty() {
        errors.push("部门名称不能为空".to_string());
    } else if dept_name.chars().count() > NAME_MAX {
        errors.push(format!("部门名称长度不能超过 {NAME_MAX} 个字符"));
    }
    if remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }
    check::check_status(status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
}

/// 收集到的错误拼接为一条消息（按字段检查顺序，可读性优于只给首条）。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 创建部门请求校验。
pub fn validate_create_dept(req: &CreateDeptReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    check_common_fields(
        (&req.dept_name, &req.remark, req.status, status_allowed),
        &mut errors,
    );
    join_errors(errors)
}

/// 更新部门请求校验。
pub fn validate_update_dept(req: &UpdateDeptReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    if req.id == 0 {
        errors.push("部门 ID 必须大于 0".to_string());
    }
    check_common_fields(
        (&req.dept_name, &req.remark, req.status, status_allowed),
        &mut errors,
    );
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreateDeptReq {
        CreateDeptReq {
            parent_id: 0,
            dept_name: "测试部门".to_string(),
            sort: 0,
            status: 1,
            allow_peer_read: 0,
            remark: "validate 层测试".to_string(),
        }
    }

    fn update_req() -> UpdateDeptReq {
        UpdateDeptReq {
            id: 1,
            parent_id: 0,
            dept_name: "测试部门".to_string(),
            sort: 0,
            status: 1,
            allow_peer_read: 0,
            remark: String::new(),
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_dept(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_empty_dept_name_reports_message() {
        let mut req = create_req();
        req.dept_name = "  ".to_string();
        let err = validate_create_dept(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("部门名称不能为空"), "实际: {err}");
    }

    #[test]
    fn create_overlong_dept_name_reports_message() {
        let mut req = create_req();
        req.dept_name = "x".repeat(65);
        let err = validate_create_dept(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("部门名称长度不能超过 64 个字符"),
            "实际: {err}"
        );
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_dept(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn create_overlong_remark_reports_message() {
        let mut req = create_req();
        req.remark = "x".repeat(256);
        let err = validate_create_dept(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("备注长度不能超过 255 个字符"), "实际: {err}");
    }

    #[test]
    fn update_zero_id_reports_message() {
        let mut req = update_req();
        req.id = 0;
        let err = validate_update_dept(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("部门 ID 必须大于 0"), "实际: {err}");
    }
}
