//! W5 迁移：数据字典两级表（类型 + 字典项），并搬迁 sys_dict 存量数据。
//!
//! 旧表 `sys_dict` 把类型与字典项压在同一行，且 `type_code` 唯一，
//! 导致一个类型只能挂一个字典项。本迁移建立 gin-vue-admin 的两级结构：
//! `sys_dictionary`（类型）1 : N `sys_dictionary_detail`（字典项）。
//!
//! 搬迁是 1 : 1 的：旧表每行 → 一个新类型 + 一个字典项。
//! `down` 不还原 `sys_dict`（1:N 压回 1:1 会丢数据）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysDictionary {
    Table,
    Id,
    Name,
    Type,
    Status,
    Remark,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysDictionaryDetail {
    Table,
    Id,
    DictionaryId,
    Label,
    Value,
    Extend,
    Sort,
    Status,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1) 字典类型表
        manager
            .create_table(
                Table::create()
                    .table(SysDictionary::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysDictionary::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Name)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Type)
                            .string_len(64)
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Remark)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysDictionary::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;

        // 2) 字典项表
        manager
            .create_table(
                Table::create()
                    .table(SysDictionaryDetail::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::DictionaryId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Label)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Value)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Extend)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Sort)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysDictionaryDetail::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_dictionary_detail_dictionary_id")
                    .table(SysDictionaryDetail::Table)
                    .col(SysDictionaryDetail::DictionaryId)
                    .col(SysDictionaryDetail::Sort)
                    .to_owned(),
            )
            .await?;

        // 3) 搬迁 sys_dict 存量数据（旧表 type_code 唯一，严格 1:1）
        let db = manager.get_connection();
        db.execute_unprepared(
            "INSERT INTO sys_dictionary (name, type, status, remark, created_at, updated_at, deleted_at)
             SELECT type_code, type_code, status, remark, created_at, updated_at, deleted_at
             FROM sys_dict",
        )
        .await?;
        db.execute_unprepared(
            "INSERT INTO sys_dictionary_detail
               (dictionary_id, label, value, extend, sort, status, created_at, updated_at, deleted_at)
             SELECT d.id, s.label, s.value, '', s.sort, s.status, s.created_at, s.updated_at, s.deleted_at
             FROM sys_dict s
             JOIN sys_dictionary d ON d.type = s.type_code",
        )
        .await?;

        // 4) 旧表退役
        manager
            .drop_table(Table::drop().table(Alias::new("sys_dict")).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysDictionaryDetail::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SysDictionary::Table).to_owned())
            .await
    }
}
