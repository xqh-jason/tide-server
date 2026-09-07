//! 公共校验小工具（与数据字典无关的通用判断）。
//!
//! 跨域复用的简单校验集中于此：状态（status）等。`status` 的**允许值**不是硬编码，
//! 由调用方从数据字典（`sys_dictionary`，`type="status"`）的启用项读取后传入——
//! 读取入口见 `modules::dictionary::service::enabled_int_values`。

/// 状态值是否在允许集合内；合法返回 `Ok(())`，否则给出「仅允许：…」的中文错误。
pub fn check_status(status: i8, allowed: &[i8]) -> Result<(), String> {
    if allowed.contains(&status) {
        Ok(())
    } else if allowed.is_empty() {
        Err("状态取值不合法".to_string())
    } else {
        let choices = allowed
            .iter()
            .map(i8::to_string)
            .collect::<Vec<_>>()
            .join(" / ");
        Err(format!("状态取值不合法，仅允许：{choices}"))
    }
}

#[cfg(test)]
mod tests {
    use super::check_status;

    #[test]
    fn allowed_value_passes() {
        assert!(check_status(1, &[0, 1]).is_ok());
    }

    #[test]
    fn disallowed_value_reports_choices() {
        let err = check_status(5, &[0, 1]).unwrap_err();
        assert_eq!(err, "状态取值不合法，仅允许：0 / 1");
    }

    #[test]
    fn empty_allowed_reports_generic() {
        let err = check_status(0, &[]).unwrap_err();
        assert_eq!(err, "状态取值不合法");
    }
}
