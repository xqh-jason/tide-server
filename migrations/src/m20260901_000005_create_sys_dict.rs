//! W4 迁移：sys_dict 数据字典表（代码生成器验证目标域）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysDict {
    Table,
    Id,
    TypeCode,
    Label,
    Value,
    Sort,
    Status,
    Remark,
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
                    .table(SysDict::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysDict::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysDict::TypeCode)
                            .string_len(64)
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(SysDict::Label).string_len(255).not_null())
                    .col(ColumnDef::new(SysDict::Value).string_len(255).not_null())
                    .col(
                        ColumnDef::new(SysDict::Sort)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDict::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysDict::Remark)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDict::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysDict::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysDict::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysDict::Table).to_owned())
            .await
    }
}
