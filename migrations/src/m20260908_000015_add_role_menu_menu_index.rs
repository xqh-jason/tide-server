//! W3 迁移 15：为 `sys_role_menu` 补充 `menu_id` 二级索引。
//!
//! 背景：关联表主键为复合 (role_id, menu_id)。软删菜单（含子孙）时按 `menu_id`
//! 批量清空角色菜单绑定（见 menu/repo.rs），该条件无法利用主键前缀，会退化为
//! 全表扫描；长事务并发场景下扫描锁会阻塞角色授权写入并可能引发死锁（同
//! sys_role_api.api_id 先例，见迁移 000014）。补 menu_id 二级索引后删除收敛为
//! 目标行的行级锁。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_role_menu_menu_id")
                    .table(SysRoleMenu::Table)
                    .col(SysRoleMenu::MenuId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_sys_role_menu_menu_id")
                    .table(SysRoleMenu::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum SysRoleMenu {
    Table,
    MenuId,
}
