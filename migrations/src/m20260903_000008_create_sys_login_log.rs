//! W5 迁移：sys_login_log 登录日志表（登录 service 自动落库）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysLoginLog {
    Table,
    Id,
    UserId,
    Username,
    Ip,
    Agent,
    Status,
    Msg,
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
                    .table(SysLoginLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysLoginLog::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::UserId)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::Username)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::Ip)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::Agent)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::Status)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::Msg)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysLoginLog::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(SysLoginLog::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_login_log_created_at")
                    .table(SysLoginLog::Table)
                    .col(SysLoginLog::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_login_log_username")
                    .table(SysLoginLog::Table)
                    .col(SysLoginLog::Username)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysLoginLog::Table).to_owned())
            .await
    }
}
