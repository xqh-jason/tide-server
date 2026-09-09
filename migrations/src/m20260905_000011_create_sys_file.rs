//! W5 迁移：sys_file 文件上传记录表（本地磁盘上传的元数据）。
//!
//! `stored_name` 全局唯一（`<uuid>.<ext>`，服务端生成），磁盘定位只认它，
//! 原始名 `name` 仅用于展示与下载响应头。列注释走 `MODIFY COLUMN`
//! （SeaORM 的 `add_column` 不产生 `COMMENT`，见 m20260904_000010 同款说明）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysFile {
    Table,
    Id,
    Name,
    StoredName,
    Ext,
    Mime,
    Size,
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
                    .table(SysFile::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysFile::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SysFile::Name).string_len(255).not_null())
                    .col(
                        ColumnDef::new(SysFile::StoredName)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysFile::Ext)
                            .string_len(20)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysFile::Mime)
                            .string_len(100)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysFile::Size)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysFile::CreatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysFile::UpdatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysFile::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysFile::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysFile::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        // 存储名唯一：防 uuid 碰撞静默覆盖，也是异常排查时的定位锚点
        manager
            .create_index(
                Index::create()
                    .name("uk_sys_file_stored_name")
                    .table(SysFile::Table)
                    .col(SysFile::StoredName)
                    .unique()
                    .to_owned(),
            )
            .await?;
        // 列表默认 created_at 倒序
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_file_created_at")
                    .table(SysFile::Table)
                    .col(SysFile::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 列注释（MODIFY 必须完整复述列定义，否则类型/默认值会被改写）
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_file` \
                   MODIFY COLUMN `name` VARCHAR(255) NOT NULL COMMENT '原始文件名（含扩展名，下载时用于 Content-Disposition）', \
                   MODIFY COLUMN `stored_name` VARCHAR(255) NOT NULL COMMENT '磁盘存储名：<uuid>.<ext>（唯一，列表按此定位文件）', \
                   MODIFY COLUMN `ext` VARCHAR(20) NOT NULL DEFAULT '' COMMENT '小写扩展名（白名单校验依据）', \
                   MODIFY COLUMN `mime` VARCHAR(100) NOT NULL DEFAULT '' COMMENT 'Content-Type', \
                   MODIFY COLUMN `size` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '字节数', \
                   MODIFY COLUMN `created_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '上传人 ID', \
                   MODIFY COLUMN `updated_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '更新人 ID'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_file` COMMENT '文件上传记录（本地磁盘存储）'")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysFile::Table).to_owned())
            .await
    }
}
