//! 创建审批基座 4 张表：模板（`hr_approval_flow`）→ 模板节点
//! （`hr_approval_flow_node`）→ 实例（`hr_approval_instance`）→ 节点记录
//! （`hr_approval_record`）。
//!
//! - `hr_approval_flow` 是业务主表（软删）：`biz_type` 单列唯一含软删占位
//!   （一个业务类型只能有一条流，删除后仍占位，避免同类型出现两条流）；
//! - 其余三张**不软删**：模板节点按 `(flow_id, seq)` 硬删重排；实例与节点记录是
//!   审批历史（单据软删时由业务层把实例置「已撤销」，不改写历史行）；
//! - 全库无物理外键：`flow_id` / `biz_id` / `applicant_id` / `approver_id` 都是逻辑外键；
//! - 幂等：逐表探测 `information_schema`，只为缺失的表建表（DDL 不在事务里，
//!   中途失败重跑可补齐剩余表）。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TABLES: [&str; 4] = [
    "hr_approval_flow",
    "hr_approval_flow_node",
    "hr_approval_instance",
    "hr_approval_record",
];

const CREATE_FLOW: &str = r#"CREATE TABLE `hr_approval_flow` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '审批流模板主键',
  `biz_type` varchar(32) NOT NULL COMMENT '业务类型（字典 approvalBizType，如 timeOff / overtime；单列唯一含软删占位）',
  `name` varchar(64) NOT NULL COMMENT '模板名称',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 启用 0 停用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_approval_flow_biz_type` (`biz_type`),
  KEY `idx_hr_approval_flow_deleted_status` (`deleted_at`, `status`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='审批流模板（一个业务类型一条）'"#;

const CREATE_FLOW_NODE: &str = r#"CREATE TABLE `hr_approval_flow_node` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '模板节点主键',
  `flow_id` bigint unsigned NOT NULL COMMENT '审批流模板 ID（hr_approval_flow.id）',
  `seq` int NOT NULL COMMENT '顺序号（从 1 递增，唯一键 (flow_id, seq)）',
  `node_name` varchar(64) NOT NULL COMMENT '节点名称',
  `node_type` tinyint NOT NULL COMMENT '节点类型：1 直属上级 2 部门负责人 3 指定用户 4 指定角色',
  `approver_ref_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '审批人引用 ID：node_type=3 时是 sys_user.id，=4 时是 sys_role.id，其余 0',
  `skip_if_empty` tinyint NOT NULL DEFAULT '0' COMMENT '解析不到审批人时是否跳过：1 跳过 0 报错（最后一个节点必须为 0）',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_approval_flow_node_seq` (`flow_id`, `seq`),
  KEY `idx_hr_approval_flow_node_flow` (`flow_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='审批流模板节点（不软删：按 (flow_id, seq) 硬删重排）'"#;

const CREATE_INSTANCE: &str = r#"CREATE TABLE `hr_approval_instance` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '审批实例主键',
  `biz_type` varchar(32) NOT NULL COMMENT '业务类型（字典 approvalBizType）',
  `biz_id` bigint unsigned NOT NULL COMMENT '业务单据 ID（软删主表 id；逻辑外键）',
  `flow_id` bigint unsigned NOT NULL COMMENT '审批流模板 ID（提交时快照，模板后续改动不影响在途实例）',
  `applicant_id` bigint unsigned NOT NULL COMMENT '申请人 sys_user.id',
  `current_seq` int NOT NULL DEFAULT '0' COMMENT '当前待审批节点顺序号；0=无可推进节点',
  `current_approver_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '当前节点审批人 sys_user.id；0=角色池或无',
  `current_approver_role_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '当前节点角色池 sys_role.id；0=非角色池',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1 审批中 2 已通过 3 已驳回 4 已撤销',
  `finished_at` datetime DEFAULT NULL COMMENT '终态时间（2/3/4 时写入）',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0=系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；0=系统写入）',
  PRIMARY KEY (`id`),
  KEY `idx_hr_approval_instance_todo` (`status`, `current_approver_id`),
  KEY `idx_hr_approval_instance_todo_role` (`status`, `current_approver_role_id`),
  KEY `idx_hr_approval_instance_biz` (`biz_type`, `biz_id`),
  KEY `idx_hr_approval_instance_applicant` (`applicant_id`, `status`),
  KEY `idx_hr_approval_instance_flow` (`flow_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='审批实例（一张单据一条；不软删，单据软删时置已撤销）'"#;

const CREATE_RECORD: &str = r#"CREATE TABLE `hr_approval_record` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '审批节点记录主键',
  `instance_id` bigint unsigned NOT NULL COMMENT '审批实例 ID（hr_approval_instance.id）',
  `seq` int NOT NULL COMMENT '节点顺序号（与模板 seq 对应）',
  `node_name` varchar(64) NOT NULL COMMENT '节点名称快照',
  `node_type` tinyint NOT NULL COMMENT '节点类型快照：1 直属上级 2 部门负责人 3 指定用户 4 指定角色',
  `approver_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '解析出的审批人 sys_user.id；0=角色池（node_type=4）或解析不到',
  `approver_ref_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '审批人引用 ID 快照（node_type=3 是 sys_user.id、=4 是 sys_role.id；角色池待办按它展开）',
  `action` tinyint NOT NULL DEFAULT '0' COMMENT '动作：0 待审批 1 通过 2 驳回 3 跳过',
  `opinion` varchar(255) NOT NULL DEFAULT '' COMMENT '审批意见',
  `acted_at` datetime DEFAULT NULL COMMENT '动作时间',
  `acted_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '实际操作人 sys_user.id（角色池下与 approver_id 不同）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_hr_approval_record_seq` (`instance_id`, `seq`),
  KEY `idx_hr_approval_record_todo` (`approver_id`, `action`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='审批节点记录（提交时一次展开；不软删）'"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        for (table, ddl) in [
            ("hr_approval_flow", CREATE_FLOW),
            ("hr_approval_flow_node", CREATE_FLOW_NODE),
            ("hr_approval_instance", CREATE_INSTANCE),
            ("hr_approval_record", CREATE_RECORD),
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
