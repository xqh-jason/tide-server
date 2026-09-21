//! 创建 HR 假期额度基座 4 张表：假期类型 + 授予批次 + 聚合账户 + 流水。
//!
//! - 账本三表（grant / balance / log）**无 `deleted_at`**：额度行不软删，
//!   作废走 `status` + 反向流水（软删占位会让 `(employee_id, leave_type_id, period)`
//!   唯一键在重建账户时冲突）；
//! - `employee_id` 指向 `hr_employee.id`（逻辑外键，全库无物理外键）；
//! - 幂等：information_schema 探测任一表存在即整体跳过。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLES: [&str; 4] = [
    "hr_leave_type",
    "hr_leave_grant",
    "hr_leave_balance",
    "hr_leave_balance_log",
];

const CREATE_LEAVE_TYPE: &str = r#"CREATE TABLE `hr_leave_type` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '假期类型主键',
  `type_code` varchar(32) NOT NULL COMMENT '类型编码（单列唯一，含软删占位）',
  `type_name` varchar(64) NOT NULL COMMENT '类型名称',
  `unit` tinyint NOT NULL DEFAULT '1' COMMENT '计量单位：1 天 2 小时',
  `balance_mode` tinyint NOT NULL DEFAULT '1' COMMENT '额度模式：1 扣额度 0 只记录不扣额度',
  `min_unit_minutes` int NOT NULL DEFAULT '240' COMMENT '最小请假单位（分钟）：240=半天 480=一天',
  `require_attachment` tinyint NOT NULL DEFAULT '0' COMMENT '是否必须上传附件：1 是 0 否',
  `allow_negative` tinyint NOT NULL DEFAULT '0' COMMENT '是否允许负余额：1 是 0 否',
  `pay_ratio` int NOT NULL DEFAULT '1000' COMMENT '计薪比例（千分比）：1000=全额 0=无薪（P5 使用）',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 启用 0 停用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_leave_type_code` (`type_code`),
  KEY `idx_hr_leave_type_deleted_status` (`deleted_at`, `status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='假期类型（带规则的业务主表，不进字典）'"#;

const CREATE_LEAVE_GRANT: &str = r#"CREATE TABLE `hr_leave_grant` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '额度授予批次主键',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（hr_employee.id）',
  `leave_type_id` bigint unsigned NOT NULL COMMENT '假期类型 ID',
  `source` tinyint NOT NULL COMMENT '来源：1 发放 2 手工调整 3 加班转调休',
  `reason` varchar(32) NOT NULL DEFAULT '' COMMENT '发放依据（字典 leaveGrantReason）',
  `period` varchar(16) NOT NULL DEFAULT '' COMMENT '归属周期（如 2026）',
  `minutes` int NOT NULL COMMENT '授予分钟数（恒正）',
  `remaining_minutes` int NOT NULL COMMENT '剩余可用分钟数',
  `effective_at` date NOT NULL COMMENT '生效日期',
  `expire_at` date DEFAULT NULL COMMENT '失效日期；NULL=永久有效',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 有效 2 已用尽 3 已失效 4 已撤销',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  KEY `idx_hr_leave_grant_fefo` (`employee_id`, `leave_type_id`, `status`, `expire_at`),
  KEY `idx_hr_leave_grant_idempotent` (`employee_id`, `leave_type_id`, `reason`, `period`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='额度授予批次（账本事实来源，不软删）'"#;

const CREATE_LEAVE_BALANCE: &str = r#"CREATE TABLE `hr_leave_balance` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '额度账户（聚合行）',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（hr_employee.id）',
  `leave_type_id` bigint unsigned NOT NULL COMMENT '假期类型 ID',
  `period` varchar(16) NOT NULL COMMENT '账期（自然年，如 2026）',
  `granted_minutes` int NOT NULL DEFAULT '0' COMMENT '累计授予',
  `used_minutes` int NOT NULL DEFAULT '0' COMMENT '累计实扣',
  `locked_minutes` int NOT NULL DEFAULT '0' COMMENT '审批中预占',
  `expired_minutes` int NOT NULL DEFAULT '0' COMMENT '累计失效作废',
  `adjust_minutes` int NOT NULL DEFAULT '0' COMMENT '手工调整净额（可负）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_leave_balance_account` (`employee_id`, `leave_type_id`, `period`),
  KEY `idx_hr_leave_balance_type` (`leave_type_id`, `period`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='额度账户聚合行（展示与扣减加锁）'"#;

const CREATE_LEAVE_BALANCE_LOG: &str = r#"CREATE TABLE `hr_leave_balance_log` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '额度流水（append-only，无软删）',
  `balance_id` bigint unsigned NOT NULL COMMENT '账户 ID',
  `employee_id` bigint unsigned NOT NULL COMMENT '员工档案 ID（冗余，便于按人查询）',
  `leave_type_id` bigint unsigned NOT NULL COMMENT '假期类型 ID',
  `grant_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '授予批次 ID；0=账户级操作',
  `biz_type` tinyint NOT NULL COMMENT '1 授予 2 手工调整 3 请假预占 4 审批实扣 5 驳回释放 6 过期作废',
  `delta_minutes` int NOT NULL COMMENT '变动分钟数（正加负减）',
  `before_minutes` int NOT NULL COMMENT '变动前可用余额',
  `after_minutes` int NOT NULL COMMENT '变动后可用余额',
  `source_kind` tinyint NOT NULL DEFAULT '0' COMMENT '0 无 1 系统任务 2 请假单 3 加班单 4 手工',
  `source_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '来源单据/批次 ID',
  `operator_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '操作人 sys_user.id；0=系统',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  PRIMARY KEY (`id`),
  KEY `idx_hr_leave_balance_log_account` (`balance_id`, `id`),
  KEY `idx_hr_leave_balance_log_employee` (`employee_id`, `leave_type_id`, `id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='额度流水（append-only：冲正靠反向记录，不改历史行）'"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        // 幂等护栏：information_schema 探测任一表是否存在，存在即整体跳过
        for table in TABLES {
            let exists = conn
                .query_one(Statement::from_string(
                    manager.get_database_backend(),
                    format!(
                        "SELECT 1 FROM information_schema.TABLES \
                         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
                    ),
                ))
                .await?
                .is_some();
            if exists {
                return Ok(());
            }
        }
        for ddl in [
            CREATE_LEAVE_TYPE,
            CREATE_LEAVE_GRANT,
            CREATE_LEAVE_BALANCE,
            CREATE_LEAVE_BALANCE_LOG,
        ] {
            conn.execute_unprepared(ddl).await?;
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
