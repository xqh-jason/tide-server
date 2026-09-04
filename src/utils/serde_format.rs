//! 时间字段序列化：统一输出 `yyyy-MM-dd HH:mm:ss` 字符串。
//!
//! `NaiveDateTime` 的 serde 默认序列化是 ISO 格式（`2026-08-25T10:17:33`），
//! 接口统一用 `#[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]`
//! 转为标准格式。

use chrono::NaiveDateTime;
use serde::Serializer;

/// 统一输出格式：`yyyy-MM-dd HH:mm:ss`
pub const DATETIME_FMT: &str = "%Y-%m-%d %H:%M:%S";

/// 格式化为 `yyyy-MM-dd HH:mm:ss`（供 DTO `From` 转换处手动调用）。
pub fn format_datetime(t: NaiveDateTime) -> String {
    t.format(DATETIME_FMT).to_string()
}

/// serde 序列化器：`NaiveDateTime` → `"yyyy-MM-dd HH:mm:ss"`。
pub fn naive_datetime<S: Serializer>(t: &NaiveDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format_datetime(*t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Sample {
        #[serde(serialize_with = "naive_datetime")]
        created_at: NaiveDateTime,
    }

    #[test]
    fn serializes_as_standard_datetime() {
        let t = NaiveDateTime::parse_from_str("2026-08-25 10:17:33", DATETIME_FMT).unwrap();
        let json = serde_json::to_string(&Sample { created_at: t }).unwrap();
        assert_eq!(json, r#"{"created_at":"2026-08-25 10:17:33"}"#);
    }
}
