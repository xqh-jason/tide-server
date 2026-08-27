//! W3 第二个迁移：为 RBAC 业务表补充中文字段注释。
//!
//! 增量修改字段注释时必须重新声明列类型和基础属性；这里刻意不声明主键、唯一键
//! 或普通索引，避免迁移重复创建/覆盖既有索引。已存在的主键和索引会由 MySQL 保留。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

fn primary_id<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.big_unsigned()
        .not_null()
        .auto_increment()
        .comment(comment);
    def
}

fn relation_id<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.big_unsigned().not_null().comment(comment);
    def
}

fn not_null_string<T>(column: T, length: u32, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.string_len(length).not_null().comment(comment);
    def
}

fn default_string<T>(column: T, length: u32, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.string_len(length)
        .not_null()
        .default("")
        .comment(comment);
    def
}

fn default_integer<T>(column: T, value: i32, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.integer().not_null().default(value).comment(comment);
    def
}

fn default_tiny_integer<T>(column: T, value: i8, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.tiny_integer()
        .not_null()
        .default(value)
        .comment(comment);
    def
}

fn created_at<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.date_time()
        .not_null()
        .default(Expr::current_timestamp())
        .comment(comment);
    def
}

fn updated_at<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.date_time()
        .not_null()
        .default(Expr::current_timestamp())
        .extra("ON UPDATE CURRENT_TIMESTAMP")
        .comment(comment);
    def
}

fn deleted_at<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.date_time().null().comment(comment);
    def
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SysUser::Table)
                    .modify_column(primary_id(SysUser::Id, "用户主键"))
                    .modify_column(not_null_string(
                        SysUser::Username,
                        50,
                        "登录用户名，全局唯一",
                    ))
                    .modify_column(not_null_string(
                        SysUser::Password,
                        100,
                        "Argon2id 密码哈希，存储 PHC 格式字符串",
                    ))
                    .modify_column(default_string(
                        SysUser::Nickname,
                        50,
                        "用户昵称，界面显示名称",
                    ))
                    .modify_column(default_string(SysUser::Phone, 20, "手机号"))
                    .modify_column(default_string(SysUser::Email, 100, "邮箱地址"))
                    .modify_column(default_string(
                        SysUser::Avatar,
                        255,
                        "头像 URL，空字符串表示未设置",
                    ))
                    .modify_column(default_tiny_integer(
                        SysUser::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(created_at(SysUser::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysUser::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(
                        SysUser::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysRole::Table)
                    .modify_column(primary_id(SysRole::Id, "角色主键"))
                    .modify_column(not_null_string(SysRole::RoleName, 50, "角色显示名称"))
                    .modify_column(not_null_string(
                        SysRole::RoleKey,
                        50,
                        "角色唯一标识；JWT roles 使用该值，super 表示超级管理员",
                    ))
                    .modify_column(default_integer(SysRole::Sort, 0, "排序值；数值越小越靠前"))
                    .modify_column(default_tiny_integer(
                        SysRole::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(default_string(SysRole::Remark, 255, "备注说明"))
                    .modify_column(created_at(SysRole::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysRole::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(
                        SysRole::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysMenu::Table)
                    .modify_column(primary_id(SysMenu::Id, "菜单/按钮主键"))
                    .modify_column(relation_id(
                        SysMenu::ParentId,
                        "上级菜单 ID；0 表示顶级节点",
                    ))
                    .modify_column(default_string(
                        SysMenu::Path,
                        255,
                        "前端路由路径；目录或菜单使用",
                    ))
                    .modify_column(default_string(
                        SysMenu::Name,
                        100,
                        "前端路由名；业务语义上全局唯一",
                    ))
                    .modify_column(default_string(
                        SysMenu::Component,
                        255,
                        "Vben 动态路由组件路径；必须能被 views glob 匹配",
                    ))
                    .modify_column(default_string(SysMenu::Title, 100, "菜单标题"))
                    .modify_column(default_string(
                        SysMenu::Icon,
                        100,
                        "菜单图标名称；空字符串表示无图标",
                    ))
                    .modify_column(default_integer(SysMenu::Sort, 0, "排序值；数值越小越靠前"))
                    .modify_column(default_tiny_integer(
                        SysMenu::KeepAlive,
                        0,
                        "页面缓存：1=开启 keep-alive，0=关闭",
                    ))
                    .modify_column(default_tiny_integer(
                        SysMenu::Hidden,
                        0,
                        "菜单可见性：1=隐藏，0=显示",
                    ))
                    .modify_column(default_tiny_integer(
                        SysMenu::MenuType,
                        2,
                        "类型：1=目录，2=菜单，3=按钮",
                    ))
                    .modify_column(default_string(
                        SysMenu::Permission,
                        100,
                        "按钮权限码；前后端授权同源的唯一事实来源",
                    ))
                    .modify_column(default_tiny_integer(
                        SysMenu::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(created_at(SysMenu::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysMenu::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(
                        SysMenu::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysApi::Table)
                    .modify_column(primary_id(SysApi::Id, "接口权限主键"))
                    .modify_column(not_null_string(
                        SysApi::Path,
                        255,
                        "后端接口路径；用于后端权限点匹配",
                    ))
                    .modify_column(not_null_string(
                        SysApi::Method,
                        10,
                        "HTTP 方法；约定大写，如 GET/POST",
                    ))
                    .modify_column(default_string(SysApi::Description, 255, "接口说明"))
                    .modify_column(default_string(
                        SysApi::ApiGroup,
                        100,
                        "接口分组，便于管理后台展示",
                    ))
                    .modify_column(default_tiny_integer(
                        SysApi::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(created_at(SysApi::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysApi::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(SysApi::DeletedAt, "软删除时间；NULL 表示未删除"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysUserRole::Table)
                    .modify_column(relation_id(
                        SysUserRole::UserId,
                        "用户 ID，联合主键，关联 sys_user.id",
                    ))
                    .modify_column(relation_id(
                        SysUserRole::RoleId,
                        "角色 ID，联合主键，关联 sys_role.id",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysRoleMenu::Table)
                    .modify_column(relation_id(
                        SysRoleMenu::RoleId,
                        "角色 ID，联合主键，关联 sys_role.id",
                    ))
                    .modify_column(relation_id(
                        SysRoleMenu::MenuId,
                        "菜单/按钮 ID，联合主键，关联 sys_menu.id",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysRoleApi::Table)
                    .modify_column(relation_id(
                        SysRoleApi::RoleId,
                        "角色 ID，联合主键，关联 sys_role.id",
                    ))
                    .modify_column(relation_id(
                        SysRoleApi::ApiId,
                        "接口权限 ID，联合主键，关联 sys_api.id",
                    ))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SysUser::Table)
                    .modify_column(primary_id(SysUser::Id, ""))
                    .modify_column(not_null_string(SysUser::Username, 50, ""))
                    .modify_column(not_null_string(SysUser::Password, 100, ""))
                    .modify_column(default_string(SysUser::Nickname, 50, ""))
                    .modify_column(default_string(SysUser::Phone, 20, ""))
                    .modify_column(default_string(SysUser::Email, 100, ""))
                    .modify_column(default_string(SysUser::Avatar, 255, ""))
                    .modify_column(default_tiny_integer(SysUser::Status, 1, ""))
                    .modify_column(created_at(SysUser::CreatedAt, ""))
                    .modify_column(updated_at(SysUser::UpdatedAt, ""))
                    .modify_column(deleted_at(SysUser::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysRole::Table)
                    .modify_column(primary_id(SysRole::Id, ""))
                    .modify_column(not_null_string(SysRole::RoleName, 50, ""))
                    .modify_column(not_null_string(SysRole::RoleKey, 50, ""))
                    .modify_column(default_integer(SysRole::Sort, 0, ""))
                    .modify_column(default_tiny_integer(SysRole::Status, 1, ""))
                    .modify_column(default_string(SysRole::Remark, 255, ""))
                    .modify_column(created_at(SysRole::CreatedAt, ""))
                    .modify_column(updated_at(SysRole::UpdatedAt, ""))
                    .modify_column(deleted_at(SysRole::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysMenu::Table)
                    .modify_column(primary_id(SysMenu::Id, ""))
                    .modify_column(relation_id(SysMenu::ParentId, ""))
                    .modify_column(default_string(SysMenu::Path, 255, ""))
                    .modify_column(default_string(SysMenu::Name, 100, ""))
                    .modify_column(default_string(SysMenu::Component, 255, ""))
                    .modify_column(default_string(SysMenu::Title, 100, ""))
                    .modify_column(default_string(SysMenu::Icon, 100, ""))
                    .modify_column(default_integer(SysMenu::Sort, 0, ""))
                    .modify_column(default_tiny_integer(SysMenu::KeepAlive, 0, ""))
                    .modify_column(default_tiny_integer(SysMenu::Hidden, 0, ""))
                    .modify_column(default_tiny_integer(SysMenu::MenuType, 2, ""))
                    .modify_column(default_string(SysMenu::Permission, 100, ""))
                    .modify_column(default_tiny_integer(SysMenu::Status, 1, ""))
                    .modify_column(created_at(SysMenu::CreatedAt, ""))
                    .modify_column(updated_at(SysMenu::UpdatedAt, ""))
                    .modify_column(deleted_at(SysMenu::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysApi::Table)
                    .modify_column(primary_id(SysApi::Id, ""))
                    .modify_column(not_null_string(SysApi::Path, 255, ""))
                    .modify_column(not_null_string(SysApi::Method, 10, ""))
                    .modify_column(default_string(SysApi::Description, 255, ""))
                    .modify_column(default_string(SysApi::ApiGroup, 100, ""))
                    .modify_column(default_tiny_integer(SysApi::Status, 1, ""))
                    .modify_column(created_at(SysApi::CreatedAt, ""))
                    .modify_column(updated_at(SysApi::UpdatedAt, ""))
                    .modify_column(deleted_at(SysApi::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysUserRole::Table)
                    .modify_column(relation_id(SysUserRole::UserId, ""))
                    .modify_column(relation_id(SysUserRole::RoleId, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysRoleMenu::Table)
                    .modify_column(relation_id(SysRoleMenu::RoleId, ""))
                    .modify_column(relation_id(SysRoleMenu::MenuId, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysRoleApi::Table)
                    .modify_column(relation_id(SysRoleApi::RoleId, ""))
                    .modify_column(relation_id(SysRoleApi::ApiId, ""))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

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
