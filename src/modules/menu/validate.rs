//! 菜单域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! status 的允许值**来自数据字典**（`type="status"` 启用项，见
//! `dictionary::service::enabled_int_values`），由调用方预取后作为
//! `status_allowed` 传入，通用判断见 `utils::check::check_status`。
//! 需要查库的规则（name 查重、存在性）留在 service 层。

use crate::modules::menu::dto::{CreateMenuReq, UpdateMenuReq};
use crate::utils::check;

/// 菜单类型白名单：`1` 目录、`2` 菜单、`3` 按钮。
const MENU_TYPES: &[i8] = &[1, 2, 3];
/// 布尔开关允许值（keep_alive / hidden 共用）。
const BOOL_VALUES: &[i8] = &[0, 1];
/// 菜单名 / 标题最大长度（对齐 `sys_menu.name/title` `VARCHAR(100)`）。
const NAME_TITLE_MAX: usize = 100;
/// path / component 长度上限（对齐 `sys_menu` 列定义 `VARCHAR(255)`）。
const PATH_MAX: usize = 255;
/// 图标名 / 按钮权限码长度上限（对齐 `sys_menu` 列定义 `VARCHAR(100)`）。
const ICON_MAX: usize = 100;
const PERMISSION_MAX: usize = 100;

/// 检查必填名称类字段：trim 后非空，且不超过列长度。
fn check_required(name: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if name.trim().is_empty() {
        errors.push(format!("{label}不能为空"));
    } else if name.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 检查可选文本字段：仅限长度（空串合法，表示未设置）。
fn check_len(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 检查 0/1 布尔开关字段。
fn check_bool_flag(v: i8, label: &str, errors: &mut Vec<String>) {
    if !BOOL_VALUES.contains(&v) {
        errors.push(format!("{label}取值不合法（仅 0 或 1）"));
    }
}

/// 检查 vben component 路径：非按钮必须 `#/views/` 开头且 `.vue` 结尾（按钮传空串放行）。
fn check_component(component: &str, menu_type: i8, errors: &mut Vec<String>) {
    if menu_type == 3 || component.is_empty() {
        return;
    }
    if !component.starts_with("#/views/") || !component.ends_with(".vue") {
        errors.push("component 必须为 #/views/xxx.vue 格式".to_string());
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

/// 菜单写请求通用字段校验（create/update 共用，不含 id）。
fn check_common_fields(
    path: &str,
    name: &str,
    component: &str,
    title: &str,
    icon: &str,
    keep_alive: i8,
    hidden: i8,
    menu_type: i8,
    permission: &str,
    status: i8,
    status_allowed: &[i8],
    errors: &mut Vec<String>,
) {
    check_required(name, "菜单路由名", NAME_TITLE_MAX, errors);
    check_required(title, "菜单标题", NAME_TITLE_MAX, errors);
    check_len(path, "菜单路由路径", PATH_MAX, errors);
    check_len(component, "component", PATH_MAX, errors);
    check_len(icon, "图标名", ICON_MAX, errors);
    check_len(permission, "按钮权限码", PERMISSION_MAX, errors);
    if !MENU_TYPES.contains(&menu_type) {
        errors.push("菜单类型取值不合法，仅允许：1（目录）、2（菜单）、3（按钮）".to_string());
    }
    check_component(component, menu_type, errors);
    check_bool_flag(keep_alive, "keep_alive", errors);
    check_bool_flag(hidden, "hidden", errors);
    check::check_status(status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
}

/// 创建菜单请求校验。
pub fn validate_create_menu(req: &CreateMenuReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    check_common_fields(
        &req.path,
        &req.name,
        &req.component,
        &req.title,
        &req.icon,
        req.keep_alive,
        req.hidden,
        req.menu_type,
        &req.permission,
        req.status,
        status_allowed,
        &mut errors,
    );
    join_errors(errors)
}

/// 更新菜单请求校验。
pub fn validate_update_menu(req: &UpdateMenuReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("菜单 ID 必须大于 0".to_string());
    }
    check_common_fields(
        &req.path,
        &req.name,
        &req.component,
        &req.title,
        &req.icon,
        req.keep_alive,
        req.hidden,
        req.menu_type,
        &req.permission,
        req.status,
        status_allowed,
        &mut errors,
    );
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreateMenuReq {
        CreateMenuReq {
            parent_id: 0,
            path: "/test".to_string(),
            name: "test_menu".to_string(),
            component: "#/views/system/test.vue".to_string(),
            title: "测试菜单".to_string(),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
        }
    }

    fn to_update_req(req: CreateMenuReq, id: u64) -> UpdateMenuReq {
        UpdateMenuReq {
            id,
            parent_id: req.parent_id,
            path: req.path,
            name: req.name,
            component: req.component,
            title: req.title,
            icon: req.icon,
            sort: req.sort,
            keep_alive: req.keep_alive,
            hidden: req.hidden,
            menu_type: req.menu_type,
            permission: req.permission,
            status: req.status,
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_menu(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_button_allows_empty_component() {
        let mut req = create_req();
        req.menu_type = 3;
        req.component.clear();
        assert!(validate_create_menu(&req, &[0, 1]).is_ok());
    }

    #[test]
    fn create_component_missing_prefix_reports_message() {
        let mut req = create_req();
        req.component = "views/system/test.vue".to_string();
        let err = validate_create_menu(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("component 必须为 #/views/xxx.vue 格式"),
            "实际: {err}"
        );
    }

    #[test]
    fn create_component_missing_suffix_reports_message() {
        let mut req = create_req();
        req.component = "#/views/system/test".to_string();
        let err = validate_create_menu(&req, &[0, 1]).unwrap_err();
        assert!(
            err.contains("component 必须为 #/views/xxx.vue 格式"),
            "实际: {err}"
        );
    }

    #[test]
    fn create_invalid_menu_type_reports_message() {
        let mut req = create_req();
        req.menu_type = 9;
        let err = validate_create_menu(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("菜单类型取值不合法"), "实际: {err}");
    }

    #[test]
    fn create_invalid_keep_alive_reports_message() {
        let mut req = create_req();
        req.keep_alive = 2;
        let err = validate_create_menu(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("keep_alive取值不合法"), "实际: {err}");
    }

    #[test]
    fn create_empty_name_reports_message() {
        let mut req = create_req();
        req.name = "  ".to_string();
        let err = validate_create_menu(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("菜单路由名不能为空"), "实际: {err}");
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_menu(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_zero_id_reports_message() {
        let req = to_update_req(create_req(), 0);
        let err = validate_update_menu(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("菜单 ID 必须大于 0"), "实际: {err}");
    }
}
