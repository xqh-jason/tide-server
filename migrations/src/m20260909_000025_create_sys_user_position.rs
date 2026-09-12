//! 用户-职位关联迁移：sys_user_position 多对多挂载表。
//!
//! 关系表**硬删除**（同 sys_user_role / sys_user_dept）；一人可挂多个职位
//! （兼任场景），职位不做「主职位」标记（与 sys_user_dept.is_primary 不同，
//! 职位纯展示，无默认归属需求）。
//! 复合主键 (user_id, position_id)；额外 position_id 索引供「按职位反查引用」
//! 使用（删除职位前的占用检查）。列注释走 `MODIFY COLUMN`（同 m20260904 说明）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysUserPosition {
    Table,
    UserId,
    PositionId,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SysUserPosition::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysUserPosition::UserId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysUserPosition::PositionId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .primary_key(Index::create().col(SysUserPosition::UserId).col(SysUserPosition::PositionId))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_user_position_position_id")
                    .table(SysUserPosition::Table)
                    .col(SysUserPosition::PositionId)
                    .to_owned(),
            )
            .await?;

        // 列注释与表注释
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_user_position` \
                   MODIFY COLUMN `user_id` BIGINT UNSIGNED NOT NULL COMMENT '用户 ID（sys_user.id）', \
                   MODIFY COLUMN `position_id` BIGINT UNSIGNED NOT NULL COMMENT '职位 ID（sys_position.id）'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_user_position` COMMENT '用户-职位关联（多对多，硬删）'")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysUserPosition::Table).to_owned())
            .await
    }
}
