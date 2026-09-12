//! 主部门唯一约束迁移（W7，批次 4 补充）。
//!
//! `sys_user_dept.is_primary` 需要「每用户至多一条 = 1」的约束，但普通唯一索引
//! 无法表达（同用户多行 `is_primary = 0` 必须允许）。这里用 MySQL 8 的**生成列**
//! 技巧：`primary_owner = IF(is_primary = 1, user_id, NULL)`，再对该列建唯一索引。
//!
//! - `is_primary = 1` 的行 → `primary_owner = user_id`，同用户第二行撞唯一键；
//! - `is_primary = 0` 的行 → `primary_owner = NULL`，多行 NULL 不参与唯一性比较。
//!
//! 该列是虚拟生成列，不参与业务读写，SeaORM 实体无需映射。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_user_dept` \
                   ADD COLUMN `primary_owner` BIGINT UNSIGNED \
                     GENERATED ALWAYS AS (IF(`is_primary` = 1, `user_id`, NULL)) VIRTUAL \
                     COMMENT '主部门占位列：is_primary=1 时为 user_id，否则 NULL（配合唯一索引）', \
                   ADD UNIQUE KEY `uk_sys_user_dept_primary` (`primary_owner`)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_user_dept` \
                   DROP INDEX `uk_sys_user_dept_primary`, \
                   DROP COLUMN `primary_owner`",
            )
            .await?;
        Ok(())
    }
}
