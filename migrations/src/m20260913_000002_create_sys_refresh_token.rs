//! 创建 `sys_refresh_token` 登录刷新凭证表（refresh token 落库 / 在线状态 / 强制下线）。
//!
//! 设计要点（spec：2026-09-13-auth-refresh-session-design.md）：
//! - refresh token 只存 SHA-256 哈希（char(64)），明文仅存在于 HttpOnly Cookie；
//! - 会话是时效数据，不套软删 `deleted_at`（按日志类处理），过期由
//!   `cleanup_user_sessions` 定时任务物理清理；
//! - `revoked_by` 遵循人字段约定：`0` = 本人登出/系统，`> 0` = 管理员 user_id。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

const CREATE_TABLE: &str = r#"CREATE TABLE `sys_refresh_token` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '会话主键',
  `user_id` bigint unsigned NOT NULL COMMENT '登录用户 ID（sys_user.id）',
  `username` varchar(64) NOT NULL DEFAULT '' COMMENT '登录用户名（冗余快照，列表页免 join）',
  `refresh_token_hash` char(64) NOT NULL COMMENT 'refresh token 的 SHA-256 十六进制哈希（不存明文）',
  `ip` varchar(64) NOT NULL DEFAULT '' COMMENT '登录 IP',
  `agent` varchar(255) NOT NULL DEFAULT '' COMMENT '登录 User-Agent（超长截断 255 字符）',
  `last_active_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '最后活跃时间（60s 节流回写；5 分钟内视为在线）',
  `expires_at` datetime NOT NULL COMMENT '会话过期时间（登录时刻 + jwt.refresh_ttl_seconds）',
  `revoked_at` datetime DEFAULT NULL COMMENT '吊销时间；NULL 表示会话有效',
  `revoked_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '吊销操作人（0=本人登出/系统，>0=管理员 user_id）',
  `revoke_reason` varchar(255) NOT NULL DEFAULT '' COMMENT '吊销原因（用户登出/管理员强制下线）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '登录时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_refresh_token_hash` (`refresh_token_hash`),
  KEY `idx_sys_refresh_token_user_id` (`user_id`),
  KEY `idx_sys_refresh_token_expires_at` (`expires_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='登录刷新凭证表（refresh token 存储 / 在线状态 / 强制下线）'"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let table_exists = conn
            .query_one(Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sys_refresh_token'",
            ))
            .await?
            .is_some();
        if table_exists {
            return Ok(());
        }
        conn.execute_unprepared(CREATE_TABLE).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS `sys_refresh_token`")
            .await?;
        Ok(())
    }
}
