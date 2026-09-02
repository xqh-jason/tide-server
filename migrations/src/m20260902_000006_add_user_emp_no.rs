//! 迁移：sys_user 表新增「工号」字段（字符类型）。
//!
//! 字符类型保持与 phone / email 一致：NOT NULL + 默认空串，存量行自动填充空串，避免旧数据 NULL。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum SysUser {
    Table,
    EmpNo,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SysUser::Table)
                    .add_column(
                        ColumnDef::new(SysUser::EmpNo)
                            .string_len(50)
                            .not_null()
                            .default("")
                            .comment("工号；员工编号，空字符串表示未设置"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SysUser::Table)
                    .drop_column(SysUser::EmpNo)
                    .to_owned(),
            )
            .await
    }
}
