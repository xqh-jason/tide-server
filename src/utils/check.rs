//! 公共校验小工具（与数据字典无关的通用判断）。
//!
//! 跨域复用的简单校验集中于此：状态（status）等。`status` 的**允许值**不是硬编码，
//! 由调用方从数据字典（`sys_dictionary`，`type="status"`）的启用项读取后传入——
//! 读取入口见 `modules::dictionary::service::enabled_int_values`。

use std::collections::HashSet;

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

pub fn duplicate_ids(ids: &[u64]) -> Vec<u64> {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();

    let mut dup_ids = Vec::new();
    for w in sorted.windows(2) {
        let id = w[0];
        if id == w[1] && dup_ids.last().copied() != Some(id) {
            dup_ids.push(id);
        }
    }
    dup_ids
}

/// 求「请求里的 id」减去「查到的 id」的差集，保持请求顺序。
///
/// 用于「请求携带的 id 集合」与「数据库实际查到（含存在性判定）的 id 集合」的比对，
/// 差集即失效 id，由调用方拼入错误文案。
pub fn collect_missing_ids(requested: &[u64], found: impl Iterator<Item = u64>) -> Vec<u64> {
    let found: HashSet<u64> = found.collect();
    requested
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect()
}

/// 拼错误文案里的 id 列表（`1, 2, 3` 形式）。
pub fn format_ids(ids: &[u64]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{check_status, collect_missing_ids, format_ids};

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

    #[test]
    fn collect_missing_ids_keeps_request_order() {
        assert_eq!(
            collect_missing_ids(&[3, 1, 4], [1, 2].into_iter()),
            vec![3, 4]
        );
    }

    #[test]
    fn collect_missing_ids_empty_when_all_found() {
        assert!(collect_missing_ids(&[1, 2], [1, 2].into_iter()).is_empty());
    }

    #[test]
    fn collect_missing_ids_empty_requested() {
        assert!(collect_missing_ids(&[], [1].into_iter()).is_empty());
    }

    #[test]
    fn format_ids_joins_with_comma() {
        assert_eq!(format_ids(&[1, 22, 333]), "1, 22, 333");
    }
}
