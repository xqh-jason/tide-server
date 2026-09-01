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

/// 字段名 → SeaORM Column 枚举标识：type_code → TypeCode、deleted_at → DeletedAt。
pub fn column_ident(name: &str) -> String {
    name.split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// 唯一字段查重函数入参类型：String → &str，其余按 rust_type 原样。
pub fn finder_param_type(rust_type: &str) -> String {
    match rust_type {
        "String" | "Text" => "&str".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_ident_converts_snake_to_pascal() {
        assert_eq!(column_ident("type_code"), "TypeCode");
        assert_eq!(column_ident("deleted_at"), "DeletedAt");
        assert_eq!(column_ident("id"), "Id");
    }

    #[test]
    fn finder_param_type_maps_string_to_str() {
        assert_eq!(finder_param_type("String"), "&str");
        assert_eq!(finder_param_type("u64"), "u64");
    }
}
