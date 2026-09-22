//! 创建考勤域 4 张表（公司排班制）：班次 / 排班 / 出勤事实 / 工作日历。
//!
//! - 只有 `hr_shift` 是软删主表（班次被历史排班引用，删了要让老排班仍能取名）；
//! - `hr_shift_schedule` / `hr_attendance_record` / `hr_work_calendar` **不软删**：
//!   它们是排班 / 事实 / 日历，写入走 upsert（软删占位会让
//!   `(employee_id, work_date)` 与 `calendar_date` 唯一键在重录时撞键）；
//! - `hr_attendance_record.external_id` 可为 NULL：`(source, external_id)` 唯一键只在
//!   第三方推送带外部 ID 时生效，MySQL 唯一键对多个 NULL 视为互不相同，故本地补录
//!   （`external_id` 为 NULL）不互相占位；
//! - 「应出勤」由「排班 × 日历」派生，不落冗余列（排班改动后事实不失真）；
//! - 幂等：逐表探测后建表。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLES: [&str; 4] = [
    "hr_shift",
    "hr_shift_schedule",
    "hr_attendance_record",
    "hr_work_calendar",
];

const CREATE_SHIFT: &str = r#"CREATE TABLE `hr_shift` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '班次主键',
  `shift_code` varchar(32) NOT NULL COMMENT '班次编码（单列唯一，含软删占位）',
  `shift_name` varchar(64) NOT NULL COMMENT '班次名称',
  `start_time` time NOT NULL COMMENT '上班时间（班次开始）',
  `end_time` time NOT NULL COMMENT '下班时间（班次结束）',
  `cross_day` tinyint NOT NULL DEFAULT '0' COMMENT '是否跨天班：1 是（end_time 落在次日）0 否',
  `work_minutes` int NOT NULL COMMENT '应工作分钟数（已扣除休息）',
  `rest_minutes` int NOT NULL DEFAULT '0' COMMENT '休息分钟数',
  `late_tolerance_minutes` int NOT NULL DEFAULT '0' COMMENT '迟到宽限分钟数',
  `need_clock` tinyint NOT NULL DEFAULT '1' COMMENT '是否需要打卡：1 需要 0 不需要',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 启用 0 停用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_shift_code` (`shift_code`),
  KEY `idx_hr_shift_deleted_status` (`deleted_at`, `status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='班次（排班制的班次定义）'"#;

const CREATE_SCHEDULE: &str = r#"CREATE TABLE `hr_shift_schedule` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '排班主键',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（hr_employee.id）',
  `work_date` date NOT NULL COMMENT '排班日期（跨天班以班次开始日为准）',
  `shift_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '班次 ID；0=当天休息',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 正常 2 已换班',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_shift_schedule_employee_date` (`employee_id`, `work_date`),
  KEY `idx_hr_shift_schedule_date` (`work_date`, `shift_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='排班（按人按日；不软删，写入即 upsert）'"#;

const CREATE_RECORD: &str = r#"CREATE TABLE `hr_attendance_record` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '出勤事实主键',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（hr_employee.id）',
  `work_date` date NOT NULL COMMENT '出勤日期（跨天班以班次开始日为准）',
  `shift_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '班次 ID 快照（排班变更不改写历史事实）；0=当天休息',
  `clock_in` datetime DEFAULT NULL COMMENT '上班打卡时间',
  `clock_out` datetime DEFAULT NULL COMMENT '下班打卡时间',
  `actual_minutes` int NOT NULL DEFAULT '0' COMMENT '实际出勤分钟数',
  `late_minutes` int NOT NULL DEFAULT '0' COMMENT '迟到分钟数',
  `early_leave_minutes` int NOT NULL DEFAULT '0' COMMENT '早退分钟数',
  `miss_clock` tinyint NOT NULL DEFAULT '0' COMMENT '缺卡：0 无 1 缺上班卡 2 缺下班卡 3 上下班卡都缺',
  `source` tinyint NOT NULL DEFAULT '1' COMMENT '数据来源：1 导入 2 手工补录 3 设备 4 钉钉 5 飞书',
  `external_id` varchar(64) DEFAULT NULL COMMENT '第三方平台的记录 ID（配合 source 判重；本地补录为 NULL）',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_attendance_record_employee_date` (`employee_id`, `work_date`),
  UNIQUE KEY `uk_hr_attendance_record_external` (`source`, `external_id`),
  KEY `idx_hr_attendance_record_date` (`work_date`, `miss_clock`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='出勤事实（按人按日；不软删，写入即 upsert）'"#;

const CREATE_CALENDAR: &str = r#"CREATE TABLE `hr_work_calendar` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '工作日历主键',
  `calendar_date` date NOT NULL COMMENT '日期（单列唯一）',
  `is_workday` tinyint NOT NULL DEFAULT '1' COMMENT '是否工作日：1 是 0 否',
  `holiday_type` tinyint NOT NULL DEFAULT '0' COMMENT '日期类型：0 普通 1 法定节假日 2 调休上班',
  `standard_minutes` int NOT NULL DEFAULT '480' COMMENT '该日标准工时（分钟）：无排班时的折算依据，默认 480',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注（如「国庆节」）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_work_calendar_date` (`calendar_date`),
  KEY `idx_hr_work_calendar_workday` (`is_workday`, `calendar_date`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='工作日历（不软删，写入即 upsert）'"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        for (table, ddl) in [
            ("hr_shift", CREATE_SHIFT),
            ("hr_shift_schedule", CREATE_SCHEDULE),
            ("hr_attendance_record", CREATE_RECORD),
            ("hr_work_calendar", CREATE_CALENDAR),
        ] {
            if !table_exists(conn, manager.get_database_backend(), table).await? {
                conn.execute_unprepared(ddl).await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        for table in TABLES.iter().rev() {
            conn.execute_unprepared(&format!("DROP TABLE IF EXISTS `{table}`"))
                .await?;
        }
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
