//! W3 迁移 16：索引命名归一化（仅改元数据，不改表结构）。
//!
//! 背景：早期迁移存在三类命名不一致（审计结论）：
//! - 列级 `.unique_key()` 自动命名（`username` / `role_key` / `type`）；
//! - 命名唯一索引漏 `sys_` 表前缀（`uk_job_name` / `uk_api_path_method`）；
//! - 普通索引漏 `sys_` 前缀（`idx_menu_parent`）。
//!
//! 统一约定：唯一索引 `uk_<表名>_<列>`，普通索引 `idx_<表名>_<列>`（表名带 `sys_`）。
//! 索引名不影响应用查询，仅为迁移/DBA/巡检的一致可读性，up/down 互逆。
//! 表名/列名/索引名均为代码内静态常量，避免动态拼接不可信输入。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// (表, 旧索引名, 新索引名, 列, 是否唯一)
const RENAMES: [(&str, &str, &str, &str, bool); 6] = [
    ("sys_user", "username", "uk_sys_user_username", "username", true),
    ("sys_role", "role_key", "uk_sys_role_role_key", "role_key", true),
    ("sys_dictionary", "type", "uk_sys_dictionary_type", "`type`", true),
    ("sys_job", "uk_job_name", "uk_sys_job_job_name", "job_name", true),
    (
        "sys_api",
        "uk_api_path_method",
        "uk_sys_api_path_method",
        "`path`, `method`",
        true,
    ),
    (
        "sys_menu",
        "idx_menu_parent",
        "idx_sys_menu_parent_id",
        "parent_id",
        false,
    ),
];

async fn apply(
    manager: &SchemaManager<'_>,
    swap: bool, // true = 旧名 -> 新名；false = 新名 -> 旧名（down）
) -> Result<(), DbErr> {
    let conn = manager.get_connection();
    for (table, old, new, columns, unique) in RENAMES {
        let (from, to) = if swap { (old, new) } else { (new, old) };
        let unique_sql = if unique { "UNIQUE" } else { "" };
        let drop_sql = format!("ALTER TABLE `{table}` DROP INDEX `{from}`");
        let add_sql = format!(
            "ALTER TABLE `{table}` ADD {unique_sql} INDEX `{to}` ({columns})"
        );
        // drop 幂等：历史环境若缺该索引则跳过（忽略错误）；重复 add 由 DDL 失败即报错兜底。
        let _ = conn.execute_unprepared(&drop_sql).await;
        conn.execute_unprepared(&add_sql).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        apply(manager, true).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        apply(manager, false).await
    }
}
