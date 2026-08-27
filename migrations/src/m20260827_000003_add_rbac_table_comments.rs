//! W3 第三个迁移：为 RBAC 业务表补充中文表注释。

use sea_orm_migration::prelude::*;

/// 表名和注释都是迁移内静态常量，避免动态拼接不可信输入。
const TABLE_COMMENTS: [(&str, &str); 7] = [
    ("sys_user", "用户表"),
    ("sys_role", "角色表"),
    ("sys_menu", "菜单与按钮权限表"),
    ("sys_api", "后端接口权限表"),
    ("sys_user_role", "用户角色关联表"),
    ("sys_role_menu", "角色菜单关联表"),
    ("sys_role_api", "角色接口关联表"),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

async fn set_table_comments(
    manager: &SchemaManager<'_>,
    comments: &[(&str, &str)],
) -> Result<(), DbErr> {
    let connection = manager.get_connection();
    for (table, comment) in comments {
        let sql = format!("ALTER TABLE `{table}` COMMENT = '{comment}'");
        connection.execute_unprepared(&sql).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        set_table_comments(manager, &TABLE_COMMENTS).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let empty_comments = TABLE_COMMENTS
            .iter()
            .map(|(table, _)| (*table, ""))
            .collect::<Vec<_>>();
        set_table_comments(manager, &empty_comments).await
    }
}
