//! 生成编排：把模板输出写入目标目录。

use std::path::Path;

use crate::def::DomainDef;

/// 生成全部文件到 `root` 下（entity/ 与 modules/<domain>/），返回写入的文件路径。
pub fn write_all(def: &DomainDef, root: &Path) -> anyhow::Result<Vec<String>> {
    let entity_dir = root.join("entity");
    let module_dir = root.join("modules").join(&def.domain);
    std::fs::create_dir_all(&entity_dir)?;
    std::fs::create_dir_all(&module_dir)?;

    let mut files = Vec::new();
    let entity_path = entity_dir.join(format!("{}.rs", def.table));
    std::fs::write(&entity_path, crate::templates::render_entity(def))?;
    files.push(entity_path.display().to_string());

    for (name, render) in [
        ("api.rs", crate::templates::render_api as fn(&DomainDef) -> String),
        ("service.rs", crate::templates::render_service),
        ("repo.rs", crate::templates::render_repo),
        ("dto.rs", crate::templates::render_dto),
        ("mod.rs", crate::templates::render_mod),
    ] {
        let path = module_dir.join(name);
        std::fs::write(&path, render(def))?;
        files.push(path.display().to_string());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::DomainDef;

    fn def() -> DomainDef {
        DomainDef::from_json(
            r#"{
                "domain": "dict", "table": "sys_dict", "comment": "数据字典",
                "fields": [
                    { "name": "id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "primary": true, "auto_increment": true },
                    { "name": "type_code", "rust_type": "String", "sql_type": "VARCHAR(64)", "unique": true }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn generate_writes_six_files() {
        let out_dir = std::env::temp_dir().join(format!("codegen_test_{}", std::process::id()));
        let files = write_all(&def(), &out_dir).unwrap();

        for name in [
            "entity/sys_dict.rs",
            "modules/dict/api.rs",
            "modules/dict/service.rs",
            "modules/dict/repo.rs",
            "modules/dict/dto.rs",
            "modules/dict/mod.rs",
        ] {
            assert!(out_dir.join(name).exists(), "缺少 {name}");
        }
        assert_eq!(files.len(), 6);

        std::fs::remove_dir_all(&out_dir).ok();
    }
}
