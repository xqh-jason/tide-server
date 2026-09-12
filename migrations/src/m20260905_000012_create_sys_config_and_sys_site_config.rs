//! W5 迁移：sys_config 参数配置表 + sys_site_config 网站设置单行表（W5-6）。
//!
//! `sys_config.config_key` 全局唯一且**含软删占位**（与 dictionary 的 type 同语义）：
//! 删除后键名仍占位，新建同键报业务错误，防历史引用歧义。
//! `sys_site_config` 恒单行 `id=1`，种子行用 `INSERT IGNORE` 幂等插入。
//! 列注释走 `MODIFY COLUMN`（SeaORM `add_column` 不产生 COMMENT，见 m20260904 同款说明）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysConfig {
    Table,
    Id,
    ConfigName,
    ConfigKey,
    ConfigValue,
    Remark,
    CreatedBy,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysSiteConfig {
    Table,
    Id,
    Name,
    Logo,
    Ico,
    WatermarkText,
    WatermarkEnable,
    WatermarkType,
    WatermarkPic,
    Mode,
    SideMode,
    Color,
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
        // 1) sys_config 参数配置表
        manager
            .create_table(
                Table::create()
                    .table(SysConfig::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysConfig::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysConfig::ConfigName)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysConfig::ConfigKey)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysConfig::ConfigValue)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysConfig::Remark)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysConfig::CreatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysConfig::UpdatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysConfig::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysConfig::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysConfig::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uk_sys_config_config_key")
                    .table(SysConfig::Table)
                    .col(SysConfig::ConfigKey)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_config_created_at")
                    .table(SysConfig::Table)
                    .col(SysConfig::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 2) sys_site_config 网站设置单行表
        manager
            .create_table(
                Table::create()
                    .table(SysSiteConfig::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysSiteConfig::Id)
                            .big_unsigned()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::Name)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::Logo)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::Ico)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::WatermarkText)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::WatermarkEnable)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::WatermarkType)
                            .string_len(16)
                            .not_null()
                            .default("text"),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::WatermarkPic)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::Mode)
                            .string_len(8)
                            .not_null()
                            .default("white"),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::SideMode)
                            .string_len(8)
                            .not_null()
                            .default("dark"),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::Color)
                            .string_len(16)
                            .not_null()
                            .default("#409EFF"),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::CreatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::UpdatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysSiteConfig::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysSiteConfig::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;

        // 3) 列注释（MODIFY 必须完整复述列定义）
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_config` \
                   MODIFY COLUMN `config_name` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '参数名称', \
                   MODIFY COLUMN `config_key` VARCHAR(64) NOT NULL COMMENT '参数键（业务内唯一，含软删占位）', \
                   MODIFY COLUMN `config_value` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '参数值', \
                   MODIFY COLUMN `remark` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '备注', \
                   MODIFY COLUMN `created_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '创建人 ID', \
                   MODIFY COLUMN `updated_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '更新人 ID'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_site_config` \
                   MODIFY COLUMN `name` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '站点名称', \
                   MODIFY COLUMN `logo` VARCHAR(255) NOT NULL DEFAULT '' COMMENT 'logo 图片 URL', \
                   MODIFY COLUMN `ico` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '浏览器 tab 图标 URL', \
                   MODIFY COLUMN `watermark_text` VARCHAR(64) NOT NULL DEFAULT '' COMMENT '水印文字', \
                   MODIFY COLUMN `watermark_enable` TINYINT(1) NOT NULL DEFAULT 0 COMMENT '水印开关 0/1', \
                   MODIFY COLUMN `watermark_type` VARCHAR(16) NOT NULL DEFAULT 'text' COMMENT '水印类型：text / pic', \
                   MODIFY COLUMN `watermark_pic` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '水印图片 URL', \
                   MODIFY COLUMN `mode` VARCHAR(8) NOT NULL DEFAULT 'white' COMMENT '主题白黑：white / black', \
                   MODIFY COLUMN `side_mode` VARCHAR(8) NOT NULL DEFAULT 'dark' COMMENT '侧边栏模式：dark / light / head', \
                   MODIFY COLUMN `color` VARCHAR(16) NOT NULL DEFAULT '#409EFF' COMMENT '主题色', \
                   MODIFY COLUMN `created_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '创建人 ID', \
                   MODIFY COLUMN `updated_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '更新人 ID'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_config` COMMENT '键值参数配置'")
            .await?;
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_site_config` COMMENT '网站设置（单行 id=1）'")
            .await?;

        // 4) 网站设置种子行：INSERT IGNORE 幂等，重复迁移不报错不重复插入
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT IGNORE INTO `sys_site_config` \
                   (id, name, logo, ico, watermark_text, watermark_enable, watermark_type, \
                    watermark_pic, mode, side_mode, color) \
                 VALUES (1, 'tide-server', '', '', '', 0, 'text', '', 'white', 'dark', '#409EFF')",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysSiteConfig::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SysConfig::Table).to_owned())
            .await
    }
}
