//! 修正 `sys_menu.permission` 列注释（仅注释，不改语义）。
//!
//! 原注释写「按钮权限码；前后端授权同源的唯一事实来源」，描述的是 2026-09-17 已
//! 连根删除的「后端也按按钮码校验」双通道设计。现状：后端授权只有接口通道
//! （`ApiPermission` × `sys_api` × `sys_role_api`），`sys_menu.permission` 只经
//! `/access-codes` 下发前端控制按钮显隐，判定面不消费。
//!
//! 与 m20260914_000001（`sys_refresh_token.revoked_by` 注释修正）同一模式：
//! baseline（m20260913_000001）文件头写明「不要修改本文件」，已建表的列注释只能
//! 靠追加迁移对齐；`down` 还原为修订前的旧文案。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const COMMENT_UP: &str = "ALTER TABLE `sys_menu` MODIFY COLUMN `permission` \
     varchar(100) NOT NULL DEFAULT '' \
     COMMENT '按钮权限码；只控前端按钮显隐，后端授权不消费（经 /access-codes 下发）'";

const COMMENT_DOWN: &str = "ALTER TABLE `sys_menu` MODIFY COLUMN `permission` \
     varchar(100) NOT NULL DEFAULT '' \
     COMMENT '按钮权限码；前后端授权同源的唯一事实来源'";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        // 幂等/安全：表不存在时跳过（与 m20260913_000001 的建表判定口径一致）
        let table_exists = conn
            .query_one(Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sys_menu'",
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
