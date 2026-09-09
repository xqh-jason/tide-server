//! 字典域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`，本模块自给），由调用方预取后作为
//! `status_allowed` 传入，通用判断见 `utils::check::check_status`。
//! 需要查库的规则（type/value 查重、类型/项存在性、类型停用判定）留在 service 层。

use crate::modules::dictionary::dto::{
    CreateDictionaryDetailReq, CreateDictionaryReq, UpdateDictionaryDetailReq, UpdateDictionaryReq,
};
use crate::utils::check;

/// 类型名称 / 编码长度上限（对齐 `sys_dictionary` 列定义 `VARCHAR(64)`）。
const NAME_TYPE_MAX: usize = 64;
/// 字典项显示文本 / 值 / 扩展字段、类型备注长度上限（对齐 `VARCHAR(255)`）。
const TEXT_MAX: usize = 255;

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

/// 创建字典类型请求校验。
pub fn validate_create_dictionary(
    req: &CreateDictionaryReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    check_required(&req.name, "字典类型名称", NAME_TYPE_MAX, &mut errors);
    check_required(&req.r#type, "字典类型编码", NAME_TYPE_MAX, &mut errors);
    check_len(&req.remark, "备注", TEXT_MAX, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

/// 更新字典类型请求校验。
pub fn validate_update_dictionary(
    req: &UpdateDictionaryReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("字典类型 ID 必须大于 0".to_string());
    }
    check_required(&req.name, "字典类型名称", NAME_TYPE_MAX, &mut errors);
    check_required(&req.r#type, "字典类型编码", NAME_TYPE_MAX, &mut errors);
    check_len(&req.remark, "备注", TEXT_MAX, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

/// 创建字典项请求校验。
pub fn validate_create_dictionary_detail(
    req: &CreateDictionaryDetailReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.dictionary_id == 0 {
        errors.push("所属字典类型 ID 必须大于 0".to_string());
    }
    check_required(&req.label, "字典项显示文本", TEXT_MAX, &mut errors);
    check_required(&req.value, "字典项值", TEXT_MAX, &mut errors);
    check_len(&req.extend, "扩展字段", TEXT_MAX, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

/// 更新字典项请求校验。
pub fn validate_update_dictionary_detail(
    req: &UpdateDictionaryDetailReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("字典项 ID 必须大于 0".to_string());
    }
    if req.dictionary_id == 0 {
        errors.push("所属字典类型 ID 必须大于 0".to_string());
    }
    check_required(&req.label, "字典项显示文本", TEXT_MAX, &mut errors);
    check_required(&req.value, "字典项值", TEXT_MAX, &mut errors);
    check_len(&req.extend, "扩展字段", TEXT_MAX, &mut errors);
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict_req() -> CreateDictionaryReq {
        CreateDictionaryReq {
            name: "测试字典".to_string(),
            r#type: "test_dict".to_string(),
            status: 1,
            remark: String::new(),
        }
    }

    fn detail_req() -> CreateDictionaryDetailReq {
        CreateDictionaryDetailReq {
            dictionary_id: 1,
            label: "启用".to_string(),
            value: "1".to_string(),
            extend: String::new(),
            sort: 0,
            status: 1,
        }
    }

    #[test]
    fn create_dictionary_valid_request_passes() {
        assert!(validate_create_dictionary(&dict_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_dictionary_empty_name_reports_message() {
        let mut req = dict_req();
        req.name = "  ".to_string();
        let err = validate_create_dictionary(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("字典类型名称不能为空"), "实际: {err}");
    }

    #[test]
    fn create_dictionary_overlong_type_reports_message() {
        let mut req = dict_req();
        req.r#type = "t".repeat(65);
        let err = validate_create_dictionary(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("字典类型编码长度不能超过 64 个字符"),
            "实际: {err}"
        );
    }

    #[test]
    fn create_dictionary_invalid_status_reports_message() {
        let mut req = dict_req();
        req.status = 9;
        let err = validate_create_dictionary(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_dictionary_zero_id_reports_message() {
        let req = UpdateDictionaryReq {
            id: 0,
            name: "测试字典".to_string(),
            r#type: "test_dict".to_string(),
            status: 1,
            remark: String::new(),
        };
        let err = validate_update_dictionary(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("字典类型 ID 必须大于 0"), "实际: {err}");
    }

    #[test]
    fn create_detail_valid_request_passes() {
        assert!(validate_create_dictionary_detail(&detail_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_detail_zero_dictionary_id_reports_message() {
        let mut req = detail_req();
        req.dictionary_id = 0;
        let err = validate_create_dictionary_detail(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("所属字典类型 ID 必须大于 0"), "实际: {err}");
    }

    #[test]
    fn create_detail_empty_value_reports_message() {
        let mut req = detail_req();
        req.value = "  ".to_string();
        let err = validate_create_dictionary_detail(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("字典项值不能为空"), "实际: {err}");
    }

    #[test]
    fn create_detail_overlong_extend_reports_message() {
        let mut req = detail_req();
        req.extend = "x".repeat(256);
        let err = validate_create_dictionary_detail(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("扩展字段长度不能超过 255 个字符"),
            "实际: {err}"
        );
    }

    #[test]
    fn update_detail_zero_id_reports_message() {
        let req = UpdateDictionaryDetailReq {
            id: 0,
            dictionary_id: 1,
            label: "启用".to_string(),
            value: "1".to_string(),
            extend: String::new(),
            sort: 0,
            status: 1,
        };
        let err = validate_update_dictionary_detail(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("字典项 ID 必须大于 0"), "实际: {err}");
    }
}
