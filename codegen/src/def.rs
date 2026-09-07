//! 域定义结构（serde 反序列化）与校验。

use serde::Deserialize;

/// 域定义：一个业务域的完整描述（对应一张主表 + 四件套）。
#[derive(Debug, Clone, Deserialize)]
pub struct DomainDef {
    pub domain: String,
    pub table: String,
    pub comment: String,
    pub fields: Vec<FieldDef>,
    #[serde(default)]
    pub unique_fields: Vec<String>,
    #[serde(default)]
    pub filters: Vec<FilterDef>,
}

/// 字段定义。
#[derive(Debug, Clone, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub rust_type: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub readonly: bool,
    /// 审计字段：由系统（repo 层）写入，不进创建/更新请求体，但要进响应体。
    #[serde(default)]
    pub audit: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// 分页过滤声明。
#[derive(Debug, Clone, Deserialize)]
pub struct FilterDef {
    pub field: String,
    pub kind: String,
}

impl DomainDef {
    /// 从 JSON 字符串解析并校验定义。
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let def: DomainDef = serde_json::from_str(json)?;
        def.validate()?;
        Ok(def)
    }

    /// 校验：必填项、主键、引用完整性、过滤类型。
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.domain.is_empty(), "domain 不能为空");
        anyhow::ensure!(!self.table.is_empty(), "table 不能为空");
        anyhow::ensure!(!self.fields.is_empty(), "fields 不能为空");
        anyhow::ensure!(
            self.fields.iter().any(|f| f.primary),
            "必须存在 primary 字段"
        );
        for name in &self.unique_fields {
            anyhow::ensure!(
                self.fields.iter().any(|f| &f.name == name),
                "unique_fields 引用不存在的字段: {name}"
            );
        }
        for f in &self.filters {
            anyhow::ensure!(
                self.fields.iter().any(|fd| &fd.name == &f.field),
                "filters 引用不存在的字段: {}",
                f.field
            );
            anyhow::ensure!(
                f.kind == "exact" || f.kind == "keyword",
                "filter kind 只能是 exact/keyword: {}",
                f.kind
            );
        }
        Ok(())
    }

    /// 表名 → 实体标识（sys_dict → sys_dict）。
    pub fn entity_ident(&self) -> String {
        self.table.clone()
    }

    /// 域名单词驼峰（dict → Dict，sys_api → SysApi）。
    pub fn camel(&self) -> String {
        self.domain
            .split('_')
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect()
    }

    /// 唯一字段名集合（字段级 unique 或 unique_fields 声明，去重排序）。
    pub fn unique_field_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .fields
            .iter()
            .filter(|f| f.unique)
            .map(|f| f.name.clone())
            .collect();
        names.extend(self.unique_fields.iter().cloned());
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_json() -> &'static str {
        r#"{
            "domain": "dict",
            "table": "sys_dict",
            "comment": "数据字典",
            "fields": [
                { "name": "id", "rust_type": "u64", "primary": true, "auto_increment": true },
                { "name": "type_code", "rust_type": "String", "unique": true }
            ],
            "unique_fields": ["type_code"],
            "filters": [ { "field": "type_code", "kind": "exact" } ]
        }"#
    }

    #[test]
    fn parses_valid_definition() {
        let def = DomainDef::from_json(valid_json()).unwrap();
        assert_eq!(def.domain, "dict");
        assert_eq!(def.table, "sys_dict");
        assert_eq!(def.fields.len(), 2);
    }

    #[test]
    fn rejects_missing_table() {
        let json = r#"{ "domain": "x", "fields": [] }"#;
        assert!(DomainDef::from_json(json).is_err());
    }

    #[test]
    fn rejects_unknown_field_in_unique() {
        let json = r#"{
            "domain": "x", "table": "t", "fields": [
                { "name": "id", "rust_type": "u64", "primary": true }
            ],
            "unique_fields": ["nope"]
        }"#;
        assert!(DomainDef::from_json(json).is_err());
    }

    #[test]
    fn rejects_unknown_field_in_filters() {
        let json = r#"{
            "domain": "x", "table": "t", "fields": [
                { "name": "id", "rust_type": "u64", "primary": true }
            ],
            "filters": [ { "field": "nope", "kind": "exact" } ]
        }"#;
        assert!(DomainDef::from_json(json).is_err());
    }
}
