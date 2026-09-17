//! sys_api 域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`），由调用方预取后作为
//! `status_allowed` 传入，通用判断见 `utils::check::check_status`。
//! 需要查库的规则（path + method 查重、存在性）留在 service 层。

use crate::modules::system::sys_api::dto::{CreateApiReq, UpdateApiReq};
use crate::utils::check;

/// HTTP 方法白名单（登记大写标准方法；授权匹配为精确比较，统一大写避免旁路）。
const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
/// path / description 长度上限（对齐 `sys_api` 列定义 `VARCHAR(255)`）。
const PATH_MAX: usize = 255;
const DESCRIPTION_MAX: usize = 255;
/// 分组名长度上限（对齐 `sys_api.api_group` `VARCHAR(100)`）。
const API_GROUP_MAX: usize = 100;

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

/// 授权角色 id 去重校验：重复入参会撞 `sys_role_api` 的 `uk(role_id, api_id)`，
/// 必须在写库前拦住（否则只能拿到 internal error）。
fn check_duplicate_role_ids(role_ids: &[u64], errors: &mut Vec<String>) {
    if !check::duplicate_ids(role_ids).is_empty() {
        errors.push("角色重复".to_string());
    }
}

/// API 写请求通用字段校验（create/update 共用，不含 id）。
fn check_common_fields(
    path: &str,
    method: &str,
    description: &str,
    api_group: &str,
    status: i8,
    status_allowed: &[i8],
    errors: &mut Vec<String>,
) {
    check_required(path, "请求路径", PATH_MAX, errors);
    if !path.starts_with('/') {
        errors.push("请求路径必须以 / 开头".to_string());
    }
    check_required(method, "HTTP 方法", 10, errors);
    if !HTTP_METHODS.contains(&method) {
        errors.push(
            "HTTP 方法不合法，仅允许：GET / POST / PUT / DELETE / PATCH / HEAD / OPTIONS"
                .to_string(),
        );
    }
    check_len(description, "描述", DESCRIPTION_MAX, errors);
    check_len(api_group, "分组名", API_GROUP_MAX, errors);
    check::check_status(status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
}

/// 创建 API 请求校验。
pub fn validate_create_api(req: &CreateApiReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    check_common_fields(
        &req.path,
        &req.method,
        &req.description,
        &req.api_group,
        req.status,
        status_allowed,
        &mut errors,
    );
    check_duplicate_role_ids(&req.role_ids, &mut errors);
    join_errors(errors)
}

/// 更新 API 请求校验。
pub fn validate_update_api(req: &UpdateApiReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("API ID 必须大于 0".to_string());
    }
    check_common_fields(
        &req.path,
        &req.method,
        &req.description,
        &req.api_group,
        req.status,
        status_allowed,
        &mut errors,
    );
    check_duplicate_role_ids(&req.role_ids, &mut errors);
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreateApiReq {
        CreateApiReq {
            path: "/api/v1/test/list".to_string(),
            method: "POST".to_string(),
            description: "validate 层测试".to_string(),
            api_group: "test".to_string(),
            status: 1,
            role_ids: vec![],
        }
    }

    fn to_update_req(req: CreateApiReq, id: u64) -> UpdateApiReq {
        UpdateApiReq {
            id,
            path: req.path,
            method: req.method,
            description: req.description,
            api_group: req.api_group,
            status: req.status,
            role_ids: req.role_ids,
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_api(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_lowercase_method_reports_message() {
        let mut req = create_req();
        req.method = "post".to_string();
        let err = validate_create_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("HTTP 方法不合法"), "实际: {err}");
    }

    #[test]
    fn create_path_without_slash_reports_message() {
        let mut req = create_req();
        req.path = "api/v1/test/list".to_string();
        let err = validate_create_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("请求路径必须以 / 开头"), "实际: {err}");
    }

    #[test]
    fn create_empty_path_reports_message() {
        let mut req = create_req();
        req.path = "  ".to_string();
        let err = validate_create_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("请求路径不能为空"), "实际: {err}");
    }

    #[test]
    fn create_overlong_path_reports_message() {
        let mut req = create_req();
        req.path = format!("/{}", "x".repeat(255));
        let err = validate_create_api(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("请求路径长度不能超过 255 个字符"),
            "实际: {err}"
        );
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_zero_id_reports_message() {
        let req = to_update_req(create_req(), 0);
        let err = validate_update_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("API ID 必须大于 0"), "实际: {err}");
    }

    /// 授权角色重复：入参重复会在关系表撞 `uk(role_id, api_id)`，必须在 validate 层拦住。
    #[test]
    fn create_duplicate_role_ids_reports_message() {
        let mut req = create_req();
        req.role_ids = vec![3, 3];
        let err = validate_create_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("角色重复"), "实际: {err}");
    }

    #[test]
    fn update_duplicate_role_ids_reports_message() {
        let mut req = to_update_req(create_req(), 1);
        req.role_ids = vec![5, 7, 5];
        let err = validate_update_api(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("角色重复"), "实际: {err}");
    }

    /// 不重复的授权列表照旧放行（去重检查不得误伤）。
    #[test]
    fn distinct_role_ids_pass() {
        let mut req = create_req();
        req.role_ids = vec![1, 2, 3];
        assert!(validate_create_api(&req, &[0, 1]).is_ok());
    }
}
