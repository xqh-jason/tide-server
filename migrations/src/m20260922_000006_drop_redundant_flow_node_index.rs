//! 删除冗余索引 `idx_hr_approval_flow_node_flow`（`hr_approval_flow_node`）。
//!
//! # 为什么是冗余的
//!
//! 建表时（`m20260922_000002`）同时建了
//! `UNIQUE KEY uk_hr_approval_flow_node_seq (flow_id, seq)` 与
//! `KEY idx_hr_approval_flow_node_flow (flow_id)`。后者是前者最左前缀的重复：
//! InnoDB 唯一二级索引同样按 B-Tree 存储，按 `flow_id` 过滤（等值 / 范围 / 排序）
//! 走唯一键即可，`flow_id` 单列索引提供不了任何额外路径。冗余索引的代价是实打实的
//! ——每次节点写入要同步维护两棵 B-Tree，还多占磁盘。
//!
//! # 幂等与可逆
//!
//! - `up`：先探测 `information_schema.STATISTICS`，索引存在才 `DROP KEY`；
//!   `STATISTICS` 无行即索引（或表）不存在，直接跳过（新库由 000002 建表时已不含该索引）。
//! - `down`：反向探测，不存在才 `ADD KEY`，可精确回滚到 000002 建表后的形态。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLE: &str = "hr_approval_flow_node";
const INDEX: &str = "idx_hr_approval_flow_node_flow";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        if index_exists(conn, manager.get_database_backend(), TABLE, INDEX).await? {
            conn.execute_unprepared(&format!("ALTER TABLE `{TABLE}` DROP KEY `{INDEX}`"))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        if !index_exists(conn, manager.get_database_backend(), TABLE, INDEX).await? {
            conn.execute_unprepared(&format!(
                "ALTER TABLE `{TABLE}` ADD KEY `{INDEX}` (`flow_id`)"
            ))
            .await?;
        }
        Ok(())
    }
}

/// 探测索引是否存在（`information_schema.STATISTICS`，限定当前库）；
/// 表不存在时该表在 `STATISTICS` 里无行，同样返回 `false`，故调用方无需单独探表。
async fn index_exists(
    conn: &impl ConnectionTrait,
    backend: sea_orm::DbBackend,
    table: &str,
    index: &str,
) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            backend,
            format!(
                "SELECT 1 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}' \
                 AND INDEX_NAME = '{index}'"
            ),
        ))
        .await?
        .is_some())
}
