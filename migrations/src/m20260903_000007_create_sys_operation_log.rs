//! W5 迁移：sys_operation_log 操作日志表（中间件自动落库）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysOperationLog {
    Table,
    Id,
    UserId,
    Ip,
    Method,
    Path,
    Status,
    Latency,
    Agent,
    Body,
    Resp,
    ErrorMessage,
    CreatedAt,
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
                    .table(SysOperationLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysOperationLog::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::UserId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Ip)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Method)
                            .string_len(16)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Path)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Status)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Latency)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Agent)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(SysOperationLog::Body).text().not_null())
                    .col(ColumnDef::new(SysOperationLog::Resp).text().not_null())
                    .col(
                        ColumnDef::new(SysOperationLog::ErrorMessage)
                            .string_len(500)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(SysOperationLog::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_operation_log_created_at")
                    .table(SysOperationLog::Table)
                    .col(SysOperationLog::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_operation_log_user_id")
                    .table(SysOperationLog::Table)
                    .col(SysOperationLog::UserId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysOperationLog::Table).to_owned())
            .await
    }
}
