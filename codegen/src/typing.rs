//! rust_type → 生成代码类型映射。

/// 定义中的 `rust_type` 原样作为 entity 字段类型；
/// `DateTime` 使用 SeaORM 实体惯例名（chrono 别名由 entity prelude 提供）。
pub fn entity_type(rust_type: &str) -> String {
    match rust_type {
        "DateTime" => "DateTime".to_string(),
        "Option<DateTime>" => "Option<DateTime>".to_string(),
        other => other.to_string(),
    }
}
