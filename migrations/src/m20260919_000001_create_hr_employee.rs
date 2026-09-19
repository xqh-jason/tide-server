//! 创建 `hr_employee` 员工档案表（HR 的第一个业务域）。
//!
//! - `user_id` 与 `sys_user.id` 做 1:1 逻辑外键（全库无物理外键）；
//! - `user_id` 单列唯一：一个平台用户终身一份档案（软删行仍占位，查重走
//!   `find_by_user_id_include_deleted`，与基座 `sys_position.position_code` 同款口径）；
//! - 审计列由 repo 层盖章；软删列 `deleted_at`；幂等靠 information_schema 探测。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLE: &str = "hr_employee";

const CREATE_TABLE: &str = r#"CREATE TABLE `hr_employee` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '员工档案主键',
  `user_id` bigint unsigned NOT NULL COMMENT '关联平台用户 ID（sys_user.id，逻辑外键）',
  `hire_date` date DEFAULT NULL COMMENT '入职日期',
  `regular_date` date DEFAULT NULL COMMENT '转正日期；NULL 表示尚未转正',
  `leave_date` date DEFAULT NULL COMMENT '离职日期；NULL 表示在职',
  `employment_status` tinyint NOT NULL DEFAULT '1' COMMENT '在职状态（字典 employmentStatus：1 在职、2 试用、3 离职）',
  `education` tinyint NOT NULL DEFAULT '0' COMMENT '最高学历（字典 education；0 未填）',
  `graduate_school` varchar(128) NOT NULL DEFAULT '' COMMENT '毕业院校',
  `major` varchar(128) NOT NULL DEFAULT '' COMMENT '所学专业',
  `id_card` varchar(32) NOT NULL DEFAULT '' COMMENT '身份证号（敏感字段，响应体脱敏）',
  `emergency_contact` varchar(64) NOT NULL DEFAULT '' COMMENT '紧急联系人',
  `emergency_phone` varchar(32) NOT NULL DEFAULT '' COMMENT '紧急联系人电话',
  `bank_account` varchar(64) NOT NULL DEFAULT '' COMMENT '工资卡号（敏感字段，响应体脱敏）',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=种子/系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_employee_user_id` (`user_id`),
  KEY `idx_hr_employee_deleted_hire` (`deleted_at`, `hire_date`),
  KEY `idx_hr_employee_deleted_status` (`deleted_at`, `employment_status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='员工档案（业务信息扩展 sys_user）'"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        // 幂等护栏：information_schema 探测表是否存在，不靠 IF NOT EXISTS 语义猜
        let exists = conn
            .query_one(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    "SELECT 1 FROM information_schema.TABLES \
                     WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{TABLE}'"
                ),
            ))
            .await?
            .is_some();
        if exists {
            return Ok(());
        }
        conn.execute_unprepared(CREATE_TABLE).await?;
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
