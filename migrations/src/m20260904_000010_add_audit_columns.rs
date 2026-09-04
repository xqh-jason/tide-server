//! W5 迁移：为 6 张配置类主表补 created_by / updated_by 审计字段。
//!
//! 存 `sys_user.id`，`NOT NULL DEFAULT 0`——`0` 表示无操作人上下文
//! （种子数据、存量数据与迁移前记录），正常业务写入必有真实 user_id。
//! 日志表与 3 张纯关联表不加：日志表已有 `user_id` 表示操作人，关联表硬删除且无审计价值。
//!
//! 列注释必须走 `MODIFY COLUMN`——SeaORM 的 `add_column` 不产生 `COMMENT`。

use sea_orm_migration::prelude::*;

/// 需要补审计字段的表：6 张由人工维护的配置类主表。
const TABLES: [&str; 6] = [
    "sys_user",
    "sys_role",
    "sys_menu",
    "sys_api",
    "sys_dictionary",
    "sys_dictionary_detail",
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in TABLES {
            // 1) 加列（一次 ALTER 带两个 ADD；NOT NULL DEFAULT 0 = 无操作人）
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new(table))
                        .add_column(
                            ColumnDef::new(Alias::new("created_by"))
                                .big_unsigned()
                                .not_null()
                                .default(0),
                        )
                        .add_column(
                            ColumnDef::new(Alias::new("updated_by"))
                                .big_unsigned()
                                .not_null()
                                .default(0),
                        )
                        .to_owned(),
                )
                .await?;

            // 2) 补列注释（MODIFY 必须完整复述列定义，否则类型/默认值会被改写）
            manager
                .get_connection()
                .execute_unprepared(&format!(
                    "ALTER TABLE `{table}` \
                       MODIFY COLUMN `created_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '创建人 ID', \
                       MODIFY COLUMN `updated_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '更新人 ID'"
                ))
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in TABLES {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new(table))
                        .drop_column(Alias::new("updated_by"))
                        .drop_column(Alias::new("created_by"))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
