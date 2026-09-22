//! P2–P4 前置断言列：`hr_employee.manager_employee_id` 与
//! `hr_time_off_grant.source_kind` / `source_id`。
//!
//! - `manager_employee_id`：审批节点「直属上级」（`node_type = 1`）的唯一解析来源，
//!   指向 `hr_employee.id`（逻辑外键，全库无物理外键；`0` = 未设置）；
//! - `source_kind` / `source_id`：批次幂等键的**来源维度**。原四列幂等键
//!   `(employee_id, time_off_type_id, reason, period)` 无法区分「同一年第二次加班转调休」，
//!   第二次入账会被幂等吞掉；`source_kind != 0` 时改用
//!   `(employee_id, time_off_type_id, source_kind, source_id)` 作幂等键。
//!
//! 幂等：逐列探测 `information_schema.COLUMNS`，只补缺失列（已存在的环境不重复 ALTER）。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `(表, 列)` → 新增列 DDL。
const COLUMNS: [(&str, &str, &str); 3] = [
    (
        "hr_employee",
        "manager_employee_id",
        "ALTER TABLE `hr_employee` ADD COLUMN `manager_employee_id` bigint unsigned NOT NULL \
         DEFAULT '0' COMMENT '直属上级员工 ID（hr_employee.id；0=未设置）' AFTER `user_id`",
    ),
    (
        "hr_time_off_grant",
        "source_kind",
        "ALTER TABLE `hr_time_off_grant` ADD COLUMN `source_kind` tinyint NOT NULL DEFAULT '0' \
         COMMENT '来源对象类型（与 hr_time_off_balance_log.source_kind 同口径）：0 无 1 系统任务 2 请假单 3 加班单 4 手工' \
         AFTER `reason`",
    ),
    (
        "hr_time_off_grant",
        "source_id",
        "ALTER TABLE `hr_time_off_grant` ADD COLUMN `source_id` bigint unsigned NOT NULL \
         DEFAULT '0' COMMENT '来源对象 ID（配合 source_kind 作幂等键）' AFTER `source_kind`",
    ),
];

/// 新增列之后补的索引（`source_kind = 0` 的行不参与，非唯一）。
const INDEX: &str = "ALTER TABLE `hr_time_off_grant` ADD KEY `idx_hr_time_off_grant_source` \
     (`employee_id`, `time_off_type_id`, `source_kind`, `source_id`)";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = manager.get_database_backend();

        for (table, column, ddl) in COLUMNS {
            if !column_exists(conn, backend, table, column).await? {
                conn.execute_unprepared(ddl).await?;
            }
        }

        let index_exists = conn
            .query_one(Statement::from_string(
                backend,
                "SELECT 1 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'hr_time_off_grant' \
                 AND INDEX_NAME = 'idx_hr_time_off_grant_source'",
            ))
            .await?
            .is_some();
        if !index_exists && table_exists(conn, backend, "hr_time_off_grant").await? {
            conn.execute_unprepared(INDEX).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = manager.get_database_backend();

        // MySQL 不支持 `DROP COLUMN IF EXISTS`（那是 MariaDB 扩展），逐列探测后再删
        for (table, column, _) in COLUMNS.iter().rev() {
            if column_exists(conn, backend, table, column).await? {
                conn.execute_unprepared(&format!("ALTER TABLE `{table}` DROP COLUMN `{column}`"))
                    .await?;
            }
        }

        let index_exists = conn
            .query_one(Statement::from_string(
                backend,
                "SELECT 1 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'hr_time_off_grant' \
                 AND INDEX_NAME = 'idx_hr_time_off_grant_source'",
            ))
            .await?
            .is_some();
        if index_exists {
            conn.execute_unprepared(
                "ALTER TABLE `hr_time_off_grant` DROP KEY `idx_hr_time_off_grant_source`",
            )
            .await?;
        }
        Ok(())
    }
}

/// 探测列是否存在（`information_schema.COLUMNS`，限定当前库）。
async fn column_exists(
    conn: &impl ConnectionTrait,
    backend: sea_orm::DbBackend,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            backend,
            format!(
                "SELECT 1 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}' \
                 AND COLUMN_NAME = '{column}'"
            ),
        ))
        .await?
        .is_some())
}

/// 探测表是否存在（`down` 与索引护栏用）。
async fn table_exists(
    conn: &impl ConnectionTrait,
    backend: sea_orm::DbBackend,
    table: &str,
) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            backend,
            format!(
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
            ),
        ))
        .await?
        .is_some())
}
