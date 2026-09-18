//! 为列表查询补缺失索引（`sys_operation_log.path` / `sys_user` 过滤列 / `sys_role.status` 等）。
//!
//! # 背景：为什么现在才加
//!
//! 2026-09-18 的审查发现「索引策略在域间不一致」：`sys_login_log` / `sys_operation_log` /
//! `sys_config` / `sys_file` 都建了 `created_at` 索引，而 `sys_user` 只有主键与唯一键；
//! `sys_operation_log.path` 更是**完全没有索引**——同批修改已把该列的查询从
//! `LIKE '%v%'`（前导通配符，索引无用）改为 `LIKE 'v%'`（前缀匹配，可用索引），
//! 但没有索引时该优化不产生任何收益（EXPLAIN 仍为全表/主键扫）。
//!
//! # 只加「查询真能用上」的索引
//!
//! 判定口径（本轮逐域核对各 `find_page` 的实际过滤形态得出）：
//!
//! - **加**：等值（`=`）、范围（`>=` / `<=`）、前缀（`LIKE 'v%'`）——这些能走 B-Tree；
//! - **不加**：前导通配符子串（`LIKE '%v%'`）——B-Tree 只能按前缀定位，
//!   前导 `%` 让索引失效，加了纯属浪费写入与磁盘。这类列有
//!   `sys_user.username`（已由 `uk_sys_user_username` 覆盖等值场景）、
//!   `sys_role.role_name` / `role_key`、`sys_menu.name`、`sys_api.path` /
//!   `description` / `api_group`、`sys_file.name`、`sys_job.job_name`、
//!   `sys_login_log.username` / `ip`、`sys_operation_log.ip`、
//!   `sys_refresh_token.username`——它们的模糊搜索目前必然全表扫，
//!   要真正优化需改查询形态（前缀匹配）或引入全文检索，不在本迁移范围。
//!
//! # 组合索引的列序
//!
//! 统一 `(deleted_at, <过滤列>)`：软删除过滤出现在**每一次**列表查询里且选择度
//! 极低（绝大多数行 `deleted_at IS NULL`），放前导位可让所有列表查询共享同一索引前缀。
//! 排序不再依赖索引——分页已统一 `ORDER BY id DESC`，走主键反向扫描。
//!
//! `down` 逐条 `DROP INDEX`，可完整回滚。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// 要创建的索引：`(表, 索引名, 列定义)`。
///
/// 单独提出来是为了 up / down 共用同一份清单，避免两边漂移。
const INDEXES: &[(&str, &str, &str)] = &[
    // 操作日志：H4-c 已改为前缀匹配，此索引是该优化的必要前提
    (
        "sys_operation_log",
        "idx_sys_operation_log_path",
        "`path`(64)",
    ),
    // 用户列表：软删过滤 + 状态/时间范围（核心表，量级最大）
    (
        "sys_user",
        "idx_sys_user_deleted_created",
        "`deleted_at`, `created_at`",
    ),
    (
        "sys_user",
        "idx_sys_user_deleted_status",
        "`deleted_at`, `status`",
    ),
    // 角色 / 菜单 / 接口：列表均支持 status 精确过滤
    (
        "sys_role",
        "idx_sys_role_deleted_status",
        "`deleted_at`, `status`",
    ),
    (
        "sys_menu",
        "idx_sys_menu_deleted_parent",
        "`deleted_at`, `parent_id`",
    ),
    (
        "sys_api",
        "idx_sys_api_deleted_status",
        "`deleted_at`, `status`",
    ),
    // 定时任务：列表支持 status 过滤
    (
        "sys_job",
        "idx_sys_job_deleted_status",
        "`deleted_at`, `status`",
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = manager.get_database_backend();

        for (table, index, cols) in INDEXES {
            // 幂等 / 安全（与 m20260917_000001 同口径）：表不存在则跳过该条
            let table_exists = conn
                .query_one(Statement::from_string(
                    backend,
                    format!(
                        "SELECT 1 FROM information_schema.TABLES \
                         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
                    ),
                ))
                .await?
                .is_some();
            if !table_exists {
                continue;
            }

            // 已存在同名索引则跳过：让本迁移可重入（对已手工加过索引的库友好）
            let index_exists = conn
                .query_one(Statement::from_string(
                    backend,
                    format!(
                        "SELECT 1 FROM information_schema.STATISTICS \
                         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}' \
                         AND INDEX_NAME = '{index}'"
                    ),
                ))
                .await?
                .is_some();
            if index_exists {
                continue;
            }

            conn.execute_unprepared(&format!(
                "ALTER TABLE `{table}` ADD INDEX `{index}` ({cols})"
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        // 逆序删除；`DROP INDEX` 对不存在的索引会报错，故先判存在
        for (table, index, _) in INDEXES.iter().rev() {
            let backend = manager.get_database_backend();
            let index_exists = conn
                .query_one(Statement::from_string(
                    backend,
                    format!(
                        "SELECT 1 FROM information_schema.STATISTICS \
                         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}' \
                         AND INDEX_NAME = '{index}'"
                    ),
                ))
                .await?
                .is_some();
            if !index_exists {
                continue;
            }
            conn.execute_unprepared(&format!("ALTER TABLE `{table}` DROP INDEX `{index}`"))
                .await?;
        }

        Ok(())
    }
}
