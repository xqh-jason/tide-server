//! 部门字段收敛迁移：删除 sys_dept 的三个冗余字段（W7）。
//!
//! - `leader`：负责人展示名。负责人已由 `sys_user_dept.is_leader` 承载（可多任、
//!   可精确关联用户），字符串快照字段语义重复且会随改名/离职过期，故删除。
//!   展示负责人由后端按 `is_leader` 拼装 `leaders` 列表。
//! - `phone` / `email`：部门联系方式。无任何读写消费方（非组织树/数据权限所需），
//!   按 YAGNI 删除。
//!
//! `down` 恢复三列原定义（含注释），保证可回滚。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_dept` \
                   DROP COLUMN `leader`, \
                   DROP COLUMN `phone`, \
                   DROP COLUMN `email`",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_dept` \
                   ADD COLUMN `leader` VARCHAR(32) NOT NULL DEFAULT '' COMMENT '负责人显示名（展示用）' AFTER `sort`, \
                   ADD COLUMN `phone` VARCHAR(32) NOT NULL DEFAULT '' COMMENT '联系电话' AFTER `leader`, \
                   ADD COLUMN `email` VARCHAR(128) NOT NULL DEFAULT '' COMMENT '邮箱' AFTER `phone`",
            )
            .await?;
        Ok(())
    }
}
