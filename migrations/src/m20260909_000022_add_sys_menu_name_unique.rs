//! `sys_menu.name` 唯一约束迁移。
//!
//! vben 的 `name` 是 keep-alive 依据，业务上要求全局唯一。此前仅靠应用层
//! 「先查后插」保证幂等，`ensure_seed` 在多进程并发（服务启动 + 另一进程直连真库
//! 跑 seed）下会 TOCTOU 重复插入菜单（2026-09-11 实际踩到）。这里补数据库唯一索引兜底。
//!
//! **软删占位语义**：与 `ensure_seed` 的查重口径一致——按 `name` 查重**不过滤**
//! `deleted_at`（软删菜单仍占用该 name，不允许重建同名菜单）。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_menu` ADD UNIQUE KEY `uk_sys_menu_name` (`name`)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_menu` DROP INDEX `uk_sys_menu_name`")
            .await?;
        Ok(())
    }
}
