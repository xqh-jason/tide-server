//! 职位迁移：sys_position 职位主数据表。
//!
//! `position_code` 全局唯一且**含软删占位**（同 sys_job.job_name / sys_dictionary.type）：
//! 唯一键不包含 `deleted_at`，软删行仍占用编码，同编码不可重建，防历史引用歧义。
//! 职位是纯主数据（不参与 RBAC 与数据权限），用户通过 sys_user_position 多值挂载。
//! 列注释走 `MODIFY COLUMN`（SeaORM `add_column` 不产生 COMMENT，同 m20260904 说明）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysPosition {
    Table,
    Id,
    PositionCode,
    PositionName,
    Sort,
    Status,
    Remark,
    CreatedBy,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SysPosition::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysPosition::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysPosition::PositionCode)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysPosition::PositionName)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysPosition::Sort)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysPosition::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysPosition::Remark)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysPosition::CreatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysPosition::UpdatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysPosition::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysPosition::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysPosition::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        // 职位编码全局唯一（含软删占位，软删行不可重建同编码）
        manager
            .create_index(
                Index::create()
                    .name("uk_sys_position_code")
                    .unique()
                    .table(SysPosition::Table)
                    .col(SysPosition::PositionCode)
                    .to_owned(),
            )
            .await?;

        // 列注释与表注释
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_position` \
                   MODIFY COLUMN `position_code` VARCHAR(64) NOT NULL COMMENT '职位编码（全局唯一，含软删占位）', \
                   MODIFY COLUMN `position_name` VARCHAR(64) NOT NULL COMMENT '职位名称', \
                   MODIFY COLUMN `sort` INT NOT NULL DEFAULT 0 COMMENT '排序值，越小越靠前', \
                   MODIFY COLUMN `status` TINYINT(1) NOT NULL DEFAULT 1 COMMENT '状态：1 启用 / 0 停用', \
                   MODIFY COLUMN `remark` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '备注', \
                   MODIFY COLUMN `created_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '创建人 ID', \
                   MODIFY COLUMN `updated_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '更新人 ID'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_position` COMMENT '职位（主数据，用户多值挂载）'")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysPosition::Table).to_owned())
            .await
    }
}
