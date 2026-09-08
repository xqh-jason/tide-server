//! W3 迁移 14：为 `sys_role_api` 补充 `api_id` 二级索引。
//!
//! 背景：关联表主键为复合 (role_id, api_id)，按 `api_id` 过滤（如软删 API 时
//! 清空该 API 的角色授权）时无法利用主键前缀，会退化为全表扫描；在长事务并发
//! 场景下扫描锁与并发插入互相冲突（MySQL 1213 死锁）。补 api_id 二级索引后，
//! 这类删除收敛为该行记录锁，避免锁扩散。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_role_api_api_id")
                    .table(SysRoleApi::Table)
                    .col(SysRoleApi::ApiId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_sys_role_api_api_id")
                    .table(SysRoleApi::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum SysRoleApi {
    Table,
    ApiId,
}
