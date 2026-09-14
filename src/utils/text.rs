//! 文本处理工具。

/// 按字符数截断字符串，避免超长文本超出 DB 列宽触发报错。
///
/// 按「字符」而非字节截断：MySQL `varchar(N)` 的 N 是字符数，且按字符切分
/// 不会把多字节字符截成非法 UTF-8（如中文、emoji）。
pub fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_keeps_prefix_within_limit() {
        assert_eq!(truncate_chars("abcdef", 3), "abc");
        assert_eq!(truncate_chars("abcdef", 6), "abcdef", "等长不应改动");
        assert_eq!(truncate_chars("abcdef", 100), "abcdef", "未超长应原样返回");
    }

    #[test]
    fn truncate_chars_counts_chars_not_bytes() {
        // 中文每字 3 字节：按字节截断会截坏，按字符应完整保留前 3 个
        assert_eq!(truncate_chars("中文截断测试", 3), "中文截");
        assert_eq!(truncate_chars("中文截断测试", 3).chars().count(), 3);
    }

    #[test]
    fn truncate_chars_handles_empty_and_zero() {
        assert_eq!(truncate_chars("", 5), "");
        assert_eq!(truncate_chars("abc", 0), "");
    }
}
