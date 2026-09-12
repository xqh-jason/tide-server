//! 用户-部门关联迁移：sys_user_dept 多对多挂载表（W7）。
//!
//! 关系表**硬删除**（同 sys_user_role）；`is_primary` 标记主部门（每用户至多一个
//! `1`，由 service 层保证），`is_leader` 标记在该部门的负责人位（数据权限直控
//! 规则 2 的凭据，可兼任多部门领导）。
//! 复合主键 (user_id, dept_id)；额外 dept_id 索引供「按部门反查引用」使用
//! （删除部门前的占用检查）。列注释走 `MODIFY COLUMN`（同 m20260904 说明）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysUserDept {
    Table,
    UserId,
    DeptId,
    IsPrimary,
    IsLeader,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SysUserDept::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysUserDept::UserId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysUserDept::DeptId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysUserDept::IsPrimary)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysUserDept::IsLeader)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .primary_key(
                        Index::create()
                            .col(SysUserDept::UserId)
                            .col(SysUserDept::DeptId),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_user_dept_dept_id")
                    .table(SysUserDept::Table)
                    .col(SysUserDept::DeptId)
                    .to_owned(),
            )
            .await?;

        // 列注释与表注释
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_user_dept` \
                   MODIFY COLUMN `user_id` BIGINT UNSIGNED NOT NULL COMMENT '用户 ID（sys_user.id）', \
                   MODIFY COLUMN `dept_id` BIGINT UNSIGNED NOT NULL COMMENT '部门 ID（sys_dept.id）', \
                   MODIFY COLUMN `is_primary` TINYINT(1) NOT NULL DEFAULT 0 COMMENT '主部门：1 是 / 0 否（每用户至多一个 1）', \
                   MODIFY COLUMN `is_leader` TINYINT(1) NOT NULL DEFAULT 0 COMMENT '本部门负责人位：1 是 / 0 否（数据权限直控凭据）'",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE `sys_user_dept` COMMENT '用户-部门关联（多对多，硬删）'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysUserDept::Table).to_owned())
            .await
    }
}
