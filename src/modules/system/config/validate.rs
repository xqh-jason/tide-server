//! config 域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! config 写请求不含 status 语义字段，不依赖数据字典白名单。
//! 需要查库的规则（config_key 查重、存在性）留在 service 层。

use crate::modules::system::config::dto::{CreateConfigReq, UpdateConfigReq, UpdateSiteConfigReq};

/// 参数名称 / 参数键长度上限（对齐 `sys_config` 列定义 `VARCHAR(64)`）。
const CONFIG_NAME_KEY_MAX: usize = 64;
/// 参数值 / 备注长度上限（对齐 `VARCHAR(255)`）。
const CONFIG_VALUE_MAX: usize = 255;
const REMARK_MAX: usize = 255;
/// 站点名称长度上限（对齐 `sys_site_config.name` `VARCHAR(64)`）。
const SITE_NAME_MAX: usize = 64;
/// logo / ico / 水印图 / 水印文字等 URL 文本长度上限（对齐 `sys_site_config` 列定义）。
const URL_MAX: usize = 255;
const WATERMARK_TEXT_MAX: usize = 64;
const COLOR_MAX: usize = 16;
/// 水印类型白名单：text 文字 / pic 图片。
const SITE_WATERMARK_TYPES: &[&str] = &["text", "pic"];
/// 主题白黑模式白名单：white / black。
const SITE_MODES: &[&str] = &["white", "black"];
/// 侧边栏模式白名单：dark / light / head。
const SITE_SIDE_MODES: &[&str] = &["dark", "light", "head"];

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

/// 键值参数写请求通用字段校验（create/update 共用，不含 id）。
fn check_config_common_fields(
    config_name: &str,
    config_key: &str,
    config_value: &str,
    remark: &str,
    errors: &mut Vec<String>,
) {
    check_required(config_name, "参数名称", CONFIG_NAME_KEY_MAX, errors);
    check_required(config_key, "参数键", CONFIG_NAME_KEY_MAX, errors);
    check_len(config_value, "参数值", CONFIG_VALUE_MAX, errors);
    check_len(remark, "备注", REMARK_MAX, errors);
}

/// 创建参数请求校验。
pub fn validate_create_config(req: &CreateConfigReq) -> Result<(), String> {
    let mut errors = Vec::new();
    check_config_common_fields(
        &req.config_name,
        &req.config_key,
        &req.config_value,
        &req.remark,
        &mut errors,
    );
    join_errors(errors)
}

/// 更新参数请求校验。
pub fn validate_update_config(req: &UpdateConfigReq) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("配置 ID 必须大于 0".to_string());
    }
    check_config_common_fields(
        &req.config_name,
        &req.config_key,
        &req.config_value,
        &req.remark,
        &mut errors,
    );
    join_errors(errors)
}

/// 网站设置更新请求校验（全量提交、恒更新 id=1）。
pub fn validate_update_site_config(req: &UpdateSiteConfigReq) -> Result<(), String> {
    let mut errors = Vec::new();
    check_required(&req.name, "站点名称", SITE_NAME_MAX, &mut errors);
    check_len(&req.logo, "logo", URL_MAX, &mut errors);
    check_len(&req.ico, "图标", URL_MAX, &mut errors);
    check_len(
        &req.watermark_text,
        "水印文字",
        WATERMARK_TEXT_MAX,
        &mut errors,
    );
    check_len(&req.watermark_pic, "水印图片", URL_MAX, &mut errors);
    if !(0..=1).contains(&req.watermark_enable) {
        errors.push("水印开关取值不合法（仅 0 或 1）".to_string());
    }
    if !SITE_WATERMARK_TYPES.contains(&req.watermark_type.as_str()) {
        errors.push("水印类型不合法，仅允许：text / pic".to_string());
    }
    if !SITE_MODES.contains(&req.mode.as_str()) {
        errors.push("主题模式不合法，仅允许：white / black".to_string());
    }
    if !SITE_SIDE_MODES.contains(&req.side_mode.as_str()) {
        errors.push("侧边栏模式不合法，仅允许：dark / light / head".to_string());
    }
    check_len(&req.color, "主题色", COLOR_MAX, &mut errors);
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_req() -> CreateConfigReq {
        CreateConfigReq {
            config_name: "测试参数".to_string(),
            config_key: "test_key".to_string(),
            config_value: "v".to_string(),
            remark: String::new(),
        }
    }

    fn site_req() -> UpdateSiteConfigReq {
        UpdateSiteConfigReq {
            name: "测试站点".to_string(),
            logo: String::new(),
            ico: String::new(),
            watermark_text: "内部系统".to_string(),
            watermark_enable: 0,
            watermark_type: "text".to_string(),
            watermark_pic: String::new(),
            mode: "white".to_string(),
            side_mode: "dark".to_string(),
            color: "#1677ff".to_string(),
        }
    }

    #[test]
    fn create_config_valid_request_passes() {
        assert!(validate_create_config(&config_req()).is_ok());
    }

    #[test]
    fn create_config_empty_name_reports_message() {
        let mut req = config_req();
        req.config_name = "  ".to_string();
        let err = validate_create_config(&req).unwrap_err();
        assert!(err.contains("参数名称不能为空"), "实际: {err}");
    }

    #[test]
    fn create_config_overlong_key_reports_message() {
        let mut req = config_req();
        req.config_key = "k".repeat(65);
        let err = validate_create_config(&req).unwrap_err();
        assert!(err.contains("参数键长度不能超过 64 个字符"), "实际: {err}");
    }

    #[test]
    fn update_config_zero_id_reports_message() {
        let req = config_req();
        let update = UpdateConfigReq {
            id: 0,
            config_name: req.config_name,
            config_key: req.config_key,
            config_value: req.config_value,
            remark: req.remark,
        };
        let err = validate_update_config(&update).unwrap_err();
        assert!(err.contains("配置 ID 必须大于 0"), "实际: {err}");
    }

    #[test]
    fn site_config_valid_request_passes() {
        assert!(validate_update_site_config(&site_req()).is_ok());
    }

    #[test]
    fn site_config_empty_name_reports_message() {
        let mut req = site_req();
        req.name = " ".to_string();
        let err = validate_update_site_config(&req).unwrap_err();
        assert!(err.contains("站点名称不能为空"), "实际: {err}");
    }

    #[test]
    fn site_config_invalid_watermark_enable_reports_message() {
        let mut req = site_req();
        req.watermark_enable = 2;
        let err = validate_update_site_config(&req).unwrap_err();
        assert!(err.contains("水印开关取值不合法"), "实际: {err}");
    }

    #[test]
    fn site_config_invalid_watermark_type_reports_message() {
        let mut req = site_req();
        req.watermark_type = "img".to_string();
        let err = validate_update_site_config(&req).unwrap_err();
        assert!(err.contains("水印类型不合法"), "实际: {err}");
    }

    #[test]
    fn site_config_invalid_mode_reports_message() {
        let mut req = site_req();
        req.mode = "light".to_string();
        let err = validate_update_site_config(&req).unwrap_err();
        assert!(err.contains("主题模式不合法"), "实际: {err}");
    }

    #[test]
    fn site_config_invalid_side_mode_reports_message() {
        let mut req = site_req();
        req.side_mode = "top".to_string();
        let err = validate_update_site_config(&req).unwrap_err();
        assert!(err.contains("侧边栏模式不合法"), "实际: {err}");
    }
}
