//! W1 第一个迁移：RBAC 5 张核心表 + 3 张关联表
//!
//! 对照 GVA（gin-vue-admin）`server/initialize/` 的表设计精简，
//! 并按 vue-vben-admin v5 契约调整：
//! - sys_menu 增加 `permission` 码字段（前端权限码 + 后端授权点唯一事实来源）
//! - 统一响应体 code=200（与 vben request.ts 的 successCode 对齐）
//! - 所有表带 created_at / updated_at / deleted_at（软删除）

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---------- 1. sys_user 用户表 ----------
        manager
            .create_table(
                Table::create()
                    .table(SysUser::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysUser::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SysUser::Username).string_len(50).unique_key().not_null())
                    .col(ColumnDef::new(SysUser::Password).string_len(100).not_null())
                    .col(ColumnDef::new(SysUser::Nickname).string_len(50).not_null().default(""))
                    .col(ColumnDef::new(SysUser::Phone).string_len(20).not_null().default(""))
                    .col(ColumnDef::new(SysUser::Email).string_len(100).not_null().default(""))
                    .col(ColumnDef::new(SysUser::Avatar).string_len(255).not_null().default(""))
                    .col(ColumnDef::new(SysUser::Status).tiny_integer().not_null().default(1))
                    .col(
                        ColumnDef::new(SysUser::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysUser::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysUser::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;

        // ---------- 2. sys_role 角色表 ----------
        manager
            .create_table(
                Table::create()
                    .table(SysRole::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysRole::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SysRole::RoleName).string_len(50).not_null())
                    .col(ColumnDef::new(SysRole::RoleKey).string_len(50).unique_key().not_null())
                    .col(ColumnDef::new(SysRole::Sort).integer().not_null().default(0))
                    .col(ColumnDef::new(SysRole::Status).tiny_integer().not_null().default(1))
                    .col(ColumnDef::new(SysRole::Remark).string_len(255).not_null().default(""))
                    .col(
                        ColumnDef::new(SysRole::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysRole::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysRole::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;

        // ---------- 3. sys_menu 菜单/按钮表 ----------
        // parent_id=0 表示顶级；menu_type: 1=目录 2=菜单 3=按钮
        // permission 为按钮权限码（模块:实体:动作），vben 的 accessCodes 与后端授权点同源
        manager
            .create_table(
                Table::create()
                    .table(SysMenu::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysMenu::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SysMenu::ParentId).big_unsigned().not_null().default(0))
                    .col(ColumnDef::new(SysMenu::Path).string_len(255).not_null().default(""))
                    .col(ColumnDef::new(SysMenu::Name).string_len(100).not_null().default(""))
                    .col(ColumnDef::new(SysMenu::Component).string_len(255).not_null().default(""))
                    .col(ColumnDef::new(SysMenu::Title).string_len(100).not_null().default(""))
                    .col(ColumnDef::new(SysMenu::Icon).string_len(100).not_null().default(""))
                    .col(ColumnDef::new(SysMenu::Sort).integer().not_null().default(0))
                    .col(ColumnDef::new(SysMenu::KeepAlive).tiny_integer().not_null().default(0))
                    .col(ColumnDef::new(SysMenu::Hidden).tiny_integer().not_null().default(0))
                    .col(ColumnDef::new(SysMenu::MenuType).tiny_integer().not_null().default(2))
                    .col(ColumnDef::new(SysMenu::Permission).string_len(100).not_null().default(""))
                    .col(ColumnDef::new(SysMenu::Status).tiny_integer().not_null().default(1))
                    .col(
                        ColumnDef::new(SysMenu::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysMenu::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysMenu::DeletedAt).date_time().null())
                    .index(Index::create().name("idx_menu_parent").col(SysMenu::ParentId))
                    .to_owned(),
            )
            .await?;

        // ---------- 4. sys_api 接口表（权限点登记） ----------
        manager
            .create_table(
                Table::create()
                    .table(SysApi::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysApi::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SysApi::Path).string_len(255).not_null())
                    .col(ColumnDef::new(SysApi::Method).string_len(10).not_null())
                    .col(ColumnDef::new(SysApi::Description).string_len(255).not_null().default(""))
                    .col(ColumnDef::new(SysApi::ApiGroup).string_len(100).not_null().default(""))
                    .col(ColumnDef::new(SysApi::Status).tiny_integer().not_null().default(1))
                    .col(
                        ColumnDef::new(SysApi::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysApi::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysApi::DeletedAt).date_time().null())
                    .index(
                        Index::create()
                            .name("uk_api_path_method")
                            .unique()
                            .col(SysApi::Path)
                            .col(SysApi::Method),
                    )
                    .to_owned(),
            )
            .await?;

        // ---------- 5. sys_user_role 用户-角色关联 ----------
        manager
            .create_table(
                Table::create()
                    .table(SysUserRole::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SysUserRole::UserId).big_unsigned().not_null())
                    .col(ColumnDef::new(SysUserRole::RoleId).big_unsigned().not_null())
                    .primary_key(Index::create().col(SysUserRole::UserId).col(SysUserRole::RoleId))
                    .to_owned(),
            )
            .await?;

        // ---------- 6. sys_role_menu 角色-菜单关联 ----------
        manager
            .create_table(
                Table::create()
                    .table(SysRoleMenu::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SysRoleMenu::RoleId).big_unsigned().not_null())
                    .col(ColumnDef::new(SysRoleMenu::MenuId).big_unsigned().not_null())
                    .primary_key(Index::create().col(SysRoleMenu::RoleId).col(SysRoleMenu::MenuId))
                    .to_owned(),
            )
            .await?;

        // ---------- 7. sys_role_api 角色-接口关联 ----------
        manager
            .create_table(
                Table::create()
                    .table(SysRoleApi::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SysRoleApi::RoleId).big_unsigned().not_null())
                    .col(ColumnDef::new(SysRoleApi::ApiId).big_unsigned().not_null())
                    .primary_key(Index::create().col(SysRoleApi::RoleId).col(SysRoleApi::ApiId))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(SysUserRole::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SysRoleMenu::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SysRoleApi::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SysApi::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SysMenu::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SysRole::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SysUser::Table).to_owned()).await?;
        Ok(())
    }
}

// ---------- 表名与列名定义 ----------

#[derive(DeriveIden)]
enum SysUser {
    Table,
    Id,
    Username,
    Password,
    Nickname,
    Phone,
    Email,
    Avatar,
    Status,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysRole {
    Table,
    Id,
    RoleName,
    RoleKey,
    Sort,
    Status,
    Remark,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysMenu {
    Table,
    Id,
    ParentId,
    Path,
    Name,
    Component,
    Title,
    Icon,
    Sort,
    KeepAlive,
    Hidden,
    MenuType,
    Permission,
    Status,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysApi {
    Table,
    Id,
    Path,
    Method,
    Description,
    ApiGroup,
    Status,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysUserRole {
    Table,
    UserId,
    RoleId,
}

#[derive(DeriveIden)]
enum SysRoleMenu {
    Table,
    RoleId,
    MenuId,
}

#[derive(DeriveIden)]
enum SysRoleApi {
    Table,
    RoleId,
    ApiId,
}
