//! 审计字段过滤的时间解析（W5 收尾增强）：列表接口的时间范围入参统一在此解析。
//!
//! 约定：接受 `yyyy-MM-dd HH:mm:ss` 与 `yyyy-MM-dd` 两种格式；格式错误 →
//! `AppError::Biz`，由 handler 渲染为业务提示而非反序列化 400。
//! 纯日期落点由 `end_of_day` 决定：范围起补 `00:00:00`、范围止补 `23:59:59`
//! （MySQL DATETIME 秒级精度下即覆盖全天），前端无需自行补秒。

use chrono::{NaiveDate, NaiveDateTime};

use crate::utils::error::AppError;

/// 解析可选时间入参，`None` / 空串透传为 `None`。
///
/// `end_of_day` 仅对纯日期入参生效（完整秒入参按字面解析）：
/// 范围起传 `false`（落 00:00:00），范围止传 `true`（落 23:59:59）。
pub fn parse_datetime(
    field: &str,
    value: &Option<String>,
    end_of_day: bool,
) -> Result<Option<NaiveDateTime>, AppError> {
    let Some(raw) = value.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    // 完整秒优先，按字面解析（起止一视同仁）
    if let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(Some(dt));
    }
    // 纯日期：NaiveDateTime 的解析器不收日期-only，走 NaiveDate 再补时刻
    match NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        Ok(d) => {
            let (h, mi, s) = if end_of_day { (23, 59, 59) } else { (0, 0, 0) };
            // 边界时刻恒合法（23:59:59 / 00:00:00 均在 NaiveDateTime 表示范围内），
            // 仅为规避 expect_used deny 而保留该错误分支
            d.and_hms_opt(h, mi, s)
                .map(Some)
                .ok_or_else(|| AppError::Biz(format!("{field} 解析失败")))
        }
        Err(_) => Err(AppError::Biz(format!(
            "{field} 格式错误，应为 yyyy-MM-dd HH:mm:ss 或 yyyy-MM-dd"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    fn full(v: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(v, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn parses_full_seconds_literal_for_both_bounds() {
        assert_eq!(
            parse_datetime("createdAtBegin", &s("2026-09-05 10:30:00"), false).unwrap(),
            Some(full("2026-09-05 10:30:00"))
        );
        assert_eq!(
            parse_datetime("createdAtEnd", &s("2026-09-05 10:30:00"), true).unwrap(),
            Some(full("2026-09-05 10:30:00"))
        );
    }

    #[test]
    fn date_only_lands_at_day_start_or_end_by_bound() {
        // 范围起：纯日期落 00:00:00（含当天起点）
        assert_eq!(
            parse_datetime("createdAtBegin", &s("2026-09-05"), false).unwrap(),
            Some(full("2026-09-05 00:00:00"))
        );
        // 范围止：纯日期落 23:59:59，否则会漏掉当天白天的数据
        assert_eq!(
            parse_datetime("createdAtEnd", &s("2026-09-05"), true).unwrap(),
            Some(full("2026-09-05 23:59:59"))
        );
    }

    #[test]
    fn none_and_blank_pass_through() {
        assert_eq!(parse_datetime("f", &None, false).unwrap(), None);
        assert_eq!(parse_datetime("f", &s("  "), true).unwrap(), None);
    }

    #[test]
    fn invalid_format_yields_biz_error() {
        let err = parse_datetime("createdAtBegin", &s("05/09/2026"), false).unwrap_err();
        assert!(matches!(err, AppError::Biz(ref m) if m.contains("格式错误")));
        assert!(err.to_string().contains("createdAtBegin"), "提示应带字段名");
    }
}
