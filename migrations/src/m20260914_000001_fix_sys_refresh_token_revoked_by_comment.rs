//! 修正 `sys_refresh_token.revoked_by` 列注释。
//!
//! `revoked_by` 语义收窄：`0` 从「本人登出 / 系统」改为「系统写入（无操作人）」，
//! 本人登出改为盖本人 user_id（`revoked_by == user_id` 即可判出自助登出）。
//!
//! 建表迁移（m20260913_000002）的 `CREATE TABLE` 文本已同步修正，本迁移负责
//! 对齐在此之前已建表的环境——已存在的表不会被重建，注释只能这样改回来。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const COMMENT_UP: &str = "ALTER TABLE `sys_refresh_token` MODIFY COLUMN `revoked_by` \
     bigint unsigned NOT NULL DEFAULT '0' \
     COMMENT '吊销操作人 user_id（本人登出=本人 id；0=系统写入）'";

const COMMENT_DOWN: &str = "ALTER TABLE `sys_refresh_token` MODIFY COLUMN `revoked_by` \
     bigint unsigned NOT NULL DEFAULT '0' \
     COMMENT '吊销操作人（0=本人登出/系统，>0=管理员 user_id）'";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        // 幂等：表不存在时跳过（与 m20260913_000002 的判定口径一致）
        let table_exists = conn
            .query_one(Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sys_refresh_token'",
            ))
            .await?
            .is_some();
        if !table_exists {
            return Ok(());
        }
        conn.execute_unprepared(COMMENT_UP).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(COMMENT_DOWN)
            .await?;
        Ok(())
    }
}
