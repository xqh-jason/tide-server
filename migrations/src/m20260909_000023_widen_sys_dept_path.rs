//! `sys_dept.dept_path` 长度扩大（255 → 512）。
//!
//! 路径段为祖先链 id（根 `/0/{id}/`、子 `/0/父id/{id}/`），深树会让路径接近甚至
//! 超过 255（每层约 3~20 字符）。本次按「约一倍」扩大，为未来更深组织树留余量。

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
                   MODIFY COLUMN `dept_path` VARCHAR(512) NOT NULL DEFAULT '' \
                   COMMENT '部门路径（根 /0/{id}/，子 /0/父id/{id}/）'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_dept` \
                   MODIFY COLUMN `dept_path` VARCHAR(255) NOT NULL DEFAULT '' \
                   COMMENT '部门路径（根 /0/{id}/，子 /0/父id/{id}/）'",
            )
            .await?;
        Ok(())
    }
}
