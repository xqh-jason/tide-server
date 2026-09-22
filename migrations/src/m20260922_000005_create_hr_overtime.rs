//! 创建 `hr_overtime_request`（加班单）。
//!
//! - 软删主表；`duration_minutes` 由后端按排班 × 工作日历派生（请求体只收起止时间）；
//! - `comp_mode = 1 转调休` 在审批通过（同一事务）时生成 `hr_time_off_grant`
//!   （`source = 3 加班转调休`、`source_kind = 3 加班单`
//!   ——`time_off::SOURCE_KIND_OVERTIME`、`source_id = 本单 ID`）；
//!   `comp_mode = 2 计加班费` 只落单据 —— P5 薪酬域不做，本期不算金额；
//! - 审批复用 `hr_approval_instance`（`biz_type = "overtime"`），不新建审批逻辑；
//! - 幂等：表存在即跳过。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLE: &str = "hr_overtime_request";

const CREATE_TABLE: &str = r#"CREATE TABLE `hr_overtime_request` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '加班单主键',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（hr_employee.id）',
  `work_date` date NOT NULL COMMENT '加班所属日期（跨天加班以班次开始日为准）',
  `start_at` datetime NOT NULL COMMENT '加班开始时间',
  `end_at` datetime NOT NULL COMMENT '加班结束时间',
  `duration_minutes` int NOT NULL COMMENT '加班分钟数（后端按排班 × 工作日历派生）',
  `overtime_type` tinyint NOT NULL COMMENT '加班类型：1 工作日 2 休息日 3 法定节假日',
  `comp_mode` tinyint NOT NULL COMMENT '补偿方式：1 转调休 2 计加班费',
  `reason` varchar(255) NOT NULL DEFAULT '' COMMENT '加班事由',
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
  KEY `idx_hr_overtime_request_employee` (`employee_id`, `status`, `work_date`),
  KEY `idx_hr_overtime_request_instance` (`approval_instance_id`),
  KEY `idx_hr_overtime_request_deleted_status` (`deleted_at`, `status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='加班单（单据表：人的意图 + 审批状态）'"#;

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
