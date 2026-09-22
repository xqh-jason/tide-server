//! 创建 `hr_time_off_request`（请假单）。
//!
//! - 软删主表（列表 / 详情逐查询过滤 `deleted_at IS NULL`）；
//! - **不存 `approver_id`**：多节点顺序审批下「谁审」无意义，审批进展从
//!   `hr_approval_instance` + `hr_approval_record` 读；
//! - `duration_minutes` 由后端按「排班 × 工作日历」派生（请求体只收起止时间）；
//! - 幂等：表存在即跳过。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLE: &str = "hr_time_off_request";

const CREATE_TABLE: &str = r#"CREATE TABLE `hr_time_off_request` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '请假单主键',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（hr_employee.id）',
  `time_off_type_id` bigint unsigned NOT NULL COMMENT '假期类型 ID（hr_time_off_type.id）',
  `start_at` datetime NOT NULL COMMENT '请假开始时间',
  `end_at` datetime NOT NULL COMMENT '请假结束时间',
  `duration_minutes` int NOT NULL COMMENT '请假分钟数（后端按排班 × 工作日历派生）',
  `reason` varchar(255) NOT NULL DEFAULT '' COMMENT '请假事由',
  `attachment_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '附件 ID（sys_file.id；0=无）',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 审批中 2 已通过 3 已驳回 4 已撤销',
  `approval_instance_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '审批实例 ID（hr_approval_instance.id；0=无）',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  KEY `idx_hr_time_off_request_employee` (`employee_id`, `status`, `start_at`),
  KEY `idx_hr_time_off_request_type` (`time_off_type_id`),
  KEY `idx_hr_time_off_request_instance` (`approval_instance_id`),
  KEY `idx_hr_time_off_request_deleted_status` (`deleted_at`, `status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='请假单（单据表：人的意图 + 审批状态）'"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        if !table_exists(conn, manager.get_database_backend(), TABLE).await? {
            conn.execute_unprepared(CREATE_TABLE).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP TABLE IF EXISTS `{TABLE}`"))
            .await?;
        Ok(())
    }
}

/// 探测表是否存在（`information_schema.TABLES`，限定当前库）。
async fn table_exists(
    conn: &impl ConnectionTrait,
    backend: sea_orm::DbBackend,
    table: &str,
) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_string(
            backend,
            format!(
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
            ),
        ))
        .await?
        .is_some())
}
