//! 审计字段过滤的时间解析（W5 收尾增强）：列表接口的时间范围入参统一在此解析。
//!
//! 约定：接受 `yyyy-MM-dd HH:mm:ss` 与 `yyyy-MM-dd` 两种格式（后者按 00:00:00 落点，
//! 范围止若只传日期需自行带出当天，建议传完整秒）；格式错误 → `AppError::Biz`，
//! 由 handler 渲染为业务提示而非反序列化 400。

use chrono::NaiveDateTime;

use crate::utils::error::AppError;

/// 解析可选时间入参（两种格式），`None` / 空串透传为 `None`。
pub fn parse_datetime(
    field: &str,
    value: &Option<String>,
) -> Result<Option<NaiveDateTime>, AppError> {
    let Some(raw) = value.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    use chrono::NaiveDate;
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| {
            // 纯日期格式：NaiveDateTime 的解析器不收，需走 NaiveDate 再补 00:00:00
            NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).expect("0 点恒合法"))
        })
        .map(Some)
        .map_err(|_| {
            AppError::Biz(format!(
                "{field} 格式错误，应为 yyyy-MM-dd HH:mm:ss 或 yyyy-MM-dd"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn parses_both_accepted_formats() {
        let full = parse_datetime("createdAtBegin", &s("2026-09-05 10:30:00")).unwrap();
        assert_eq!(
            full,
            Some(
                NaiveDateTime::parse_from_str("2026-09-05 10:30:00", "%Y-%m-%d %H:%M:%S").unwrap()
            )
        );
        let date_only = parse_datetime("createdAtBegin", &s("2026-09-05")).unwrap();
        assert_eq!(
            date_only,
            Some(
                NaiveDateTime::parse_from_str("2026-09-05 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
            )
        );
    }

    #[test]
    fn none_and_blank_pass_through() {
        assert_eq!(parse_datetime("f", &None).unwrap(), None);
        assert_eq!(parse_datetime("f", &s("  ")).unwrap(), None);
    }

    #[test]
    fn invalid_format_yields_biz_error() {
        let err = parse_datetime("createdAtBegin", &s("05/09/2026")).unwrap_err();
        assert!(matches!(err, AppError::Biz(ref m) if m.contains("格式错误")));
        assert!(err.to_string().contains("createdAtBegin"), "提示应带字段名");
    }
}
