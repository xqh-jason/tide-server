//! 恢复 `sys_menu.parent_id` 的默认值。
//!
//! 字段注释迁移重定义列时意外移除了初始建表里的 `DEFAULT 0`，导致插入顶级
//! 按钮或菜单时必须显式提供 `parent_id`。这里恢复原设计；回滚仅删除默认值，
//! 不改回更早的错误状态以外的字段定义。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

async fn restore_parent_default(manager: &SchemaManager<'_>, sql: &str) -> Result<(), DbErr> {
    manager.get_connection().execute_unprepared(sql).await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        restore_parent_default(
            manager,
            "ALTER TABLE `sys_menu`
                MODIFY COLUMN `parent_id` BIGINT UNSIGNED NOT NULL DEFAULT 0
                COMMENT '上级菜单 ID；0 表示顶级节点'",
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        restore_parent_default(
            manager,
            "ALTER TABLE `sys_menu`
                MODIFY COLUMN `parent_id` BIGINT UNSIGNED NOT NULL
                COMMENT '上级菜单 ID；0 表示顶级节点'",
        )
        .await
    }
}
