//! 部门迁移：sys_dept 组织部门树表（W7）。
//!
//! `dept_path` 段为从根占位 `0` 到**自身**的整条部门 id 链：根 `/0/{id}/`、
//! 子 `/0/父id/{id}/`；移动节点后整棵子树 path 需按新父链重算（见 dept repo
//! `move_subtree_in_tx`）。
//!
//! `dept_name` + `parent_id` 复合唯一且**含软删占位**（同 sys_config.config_key）：
//! 软删行仍占用唯一键，同父同名不可重建，防历史引用歧义。
//! 列注释走 `MODIFY COLUMN`（SeaORM `add_column` 不产生 COMMENT，同 m20260904 说明）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysDept {
    Table,
    Id,
    ParentId,
    DeptPath,
    DeptName,
    Sort,
    Leader,
    Phone,
    Email,
    Status,
    AllowPeerRead,
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
                    .table(SysDept::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysDept::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysDept::ParentId)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDept::DeptPath)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(SysDept::DeptName).string_len(64).not_null())
                    .col(
                        ColumnDef::new(SysDept::Sort)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDept::Leader)
                            .string_len(32)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDept::Phone)
                            .string_len(32)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDept::Email)
                            .string_len(128)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDept::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysDept::AllowPeerRead)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDept::Remark)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDept::CreatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDept::UpdatedBy)
                            .big_unsigned()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDept::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysDept::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysDept::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        // 同父部门名唯一（含软删占位）；(parent_id) 前缀同时供 find_children 使用
        manager
            .create_index(
                Index::create()
                    .name("uk_sys_dept_parent_name")
                    .unique()
                    .table(SysDept::Table)
                    .col(SysDept::ParentId)
                    .col(SysDept::DeptName)
                    .to_owned(),
            )
            .await?;

        // 列注释与表注释
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_dept` \
                   MODIFY COLUMN `parent_id` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '父部门 ID，0 表示根部门', \
                   MODIFY COLUMN `dept_path` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '部门路径（根 /0/{id}/，子 /0/父id/{id}/）', \
                   MODIFY COLUMN `dept_name` VARCHAR(64) NOT NULL COMMENT '部门名（同父唯一，含软删占位）', \
                   MODIFY COLUMN `sort` INT NOT NULL DEFAULT 0 COMMENT '排序值，越小越靠前', \
                   MODIFY COLUMN `leader` VARCHAR(32) NOT NULL DEFAULT '' COMMENT '负责人显示名（展示用）', \
                   MODIFY COLUMN `phone` VARCHAR(32) NOT NULL DEFAULT '' COMMENT '联系电话', \
                   MODIFY COLUMN `email` VARCHAR(128) NOT NULL DEFAULT '' COMMENT '邮箱', \
                   MODIFY COLUMN `status` TINYINT(1) NOT NULL DEFAULT 1 COMMENT '状态：1 启用 / 0 停用', \
                   MODIFY COLUMN `allow_peer_read` TINYINT(1) NOT NULL DEFAULT 0 COMMENT '同级互看：1 同部门普通成员可互看 / 0 关', \
                   MODIFY COLUMN `remark` VARCHAR(255) NOT NULL DEFAULT '' COMMENT '备注', \
                   MODIFY COLUMN `created_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '创建人 ID', \
                   MODIFY COLUMN `updated_by` BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT '更新人 ID'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_dept` COMMENT '部门（组织树，数据权限直控挂载点）'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysDept::Table).to_owned())
            .await
    }
}
