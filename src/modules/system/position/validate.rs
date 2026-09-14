//! 职位域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`），由调用方预取后作为
//! `status_allowed` 传入，通用判断见 `utils::check::check_status`。
//! 需要查库的规则（code 查重、职位存在性、删除引用检查）留在 service 层。

use crate::modules::system::position::dto::{CreatePositionReq, UpdatePositionReq};
use crate::utils::check;

/// 编码 / 名称长度上限（对齐 `sys_position` 列定义 `VARCHAR(64)`）。
const CODE_NAME_MAX: usize = 64;
/// 备注长度上限（对齐 `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;

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

/// 收集到的错误拼接为一条消息（按字段检查顺序，可读性优于只给首条）。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 创建职位请求校验。
pub fn validate_create_position(
    req: &CreatePositionReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    check_required(&req.position_code, "职位编码", CODE_NAME_MAX, &mut errors);
    check_required(&req.position_name, "职位名称", CODE_NAME_MAX, &mut errors);
    check_len(&req.remark, "备注", REMARK_MAX, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

/// 更新职位请求校验。
pub fn validate_update_position(
    req: &UpdatePositionReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("职位 ID 必须大于 0".to_string());
    }
    check_required(&req.position_code, "职位编码", CODE_NAME_MAX, &mut errors);
    check_required(&req.position_name, "职位名称", CODE_NAME_MAX, &mut errors);
    check_len(&req.remark, "备注", REMARK_MAX, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreatePositionReq {
        CreatePositionReq {
            position_code: "P01".to_string(),
            position_name: "总经理".to_string(),
            sort: 0,
            status: 1,
            remark: String::new(),
        }
    }

    fn update_req() -> UpdatePositionReq {
        UpdatePositionReq {
            id: 1,
            position_code: "P01".to_string(),
            position_name: "总经理".to_string(),
            sort: 0,
            status: 1,
            remark: String::new(),
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_position(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_blank_code_reports_message() {
        let mut req = create_req();
        req.position_code = "  ".to_string();
        let err = validate_create_position(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("职位编码不能为空"), "实际: {err}");
    }

    #[test]
    fn create_blank_name_reports_message() {
        let mut req = create_req();
        req.position_name = String::new();
        let err = validate_create_position(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("职位名称不能为空"), "实际: {err}");
    }

    #[test]
    fn create_too_long_code_reports_message() {
        let mut req = create_req();
        req.position_code = "x".repeat(65);
        let err = validate_create_position(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("职位编码长度不能超过 64"), "实际: {err}");
    }

    #[test]
    fn create_too_long_remark_reports_message() {
        let mut req = create_req();
        req.remark = "备".repeat(256);
        let err = validate_create_position(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("备注长度不能超过 255"), "实际: {err}");
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_position(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_zero_id_reports_message() {
        let mut req = update_req();
        req.id = 0;
        let err = validate_update_position(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("职位 ID 必须大于 0"), "实际: {err}");
    }

    #[test]
    fn update_multiple_errors_joined() {
        let mut req = update_req();
        req.position_code = String::new();
        req.position_name = String::new();
        let err = validate_update_position(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("职位编码不能为空") && err.contains("职位名称不能为空"),
            "多条错误应以「；」拼接，实际: {err}"
        );
        assert!(err.contains('；'), "分隔符应为中文分号，实际: {err}");
    }
}
