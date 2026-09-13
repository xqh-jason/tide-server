//! 基线迁移：一次性创建全部 20 张业务表的最终 schema（含表/列注释与索引）。
//!
//! 由 2026-08-25 ~ 2026-09-09 的 25 个历史迁移压平而来，DDL 取自其最终态
//! （空库应用旧迁移后 mysqldump --no-data 导出）；后续 schema 变更继续追加
//! 新迁移文件，不要修改本文件。
//!
//! 幂等护栏：通过「sys_user 表是否已存在」判定——曾应用旧 25 条迁移的库
//! 直接视为已建表、跳过本迁移；仅全新库执行全量建表。新旧库的 `migration up`
//! 因此都可安全重入，旧版本号残留在 seaql_migrations 中无害。
//!
//! 数据种子：`sys_site_config` 恒单行 id=1（沿用原 000012 的 INSERT IGNORE）；
//! 字典、菜单、RBAC 等业务数据由应用层 `infra::seed` 启动时幂等填充。

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// 全部建表 DDL（最终态，顺序无关：表间无外键约束）
const CREATE_TABLES: &[&str] = &[
    r#"CREATE TABLE `sys_api` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '接口权限主键',
  `path` varchar(255) NOT NULL COMMENT '后端接口路径；用于后端权限点匹配',
  `method` varchar(10) NOT NULL COMMENT 'HTTP 方法；约定大写，如 GET/POST',
  `description` varchar(255) NOT NULL DEFAULT '' COMMENT '接口说明',
  `api_group` varchar(100) NOT NULL DEFAULT '' COMMENT '接口分组，便于管理后台展示',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_api_path_method` (`path`,`method`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='后端接口权限表';"#,
    r#"CREATE TABLE `sys_config` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '参数主键',
  `config_name` varchar(64) NOT NULL DEFAULT '' COMMENT '参数名称',
  `config_key` varchar(64) NOT NULL COMMENT '参数键（业务内唯一，含软删占位）',
  `config_value` varchar(255) NOT NULL DEFAULT '' COMMENT '参数值',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_config_config_key` (`config_key`),
  KEY `idx_sys_config_created_at` (`created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='键值参数配置';"#,
    r#"CREATE TABLE `sys_dept` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `parent_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '父部门 ID，0 表示根部门',
  `dept_path` varchar(512) NOT NULL DEFAULT '' COMMENT '部门路径（根 /0/{id}/，子 /0/父id/{id}/）',
  `dept_name` varchar(64) NOT NULL COMMENT '部门名（同父唯一，含软删占位）',
  `sort` int NOT NULL DEFAULT '0' COMMENT '排序值，越小越靠前',
  `status` tinyint(1) NOT NULL DEFAULT '1' COMMENT '状态：1 启用 / 0 停用',
  `allow_peer_read` tinyint(1) NOT NULL DEFAULT '0' COMMENT '同级互看：1 同部门普通成员可互看 / 0 关',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  `deleted_at` datetime DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_dept_parent_name` (`parent_id`,`dept_name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='部门（组织树，数据权限直控挂载点）';"#,
    r#"CREATE TABLE `sys_dictionary` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '字典类型主键',
  `name` varchar(64) NOT NULL DEFAULT '' COMMENT '字典名称（显示名）',
  `type` varchar(64) NOT NULL COMMENT '字典类型编码（全局唯一，含软删占位；SQL 列名 type 为保留字）',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_dictionary_type` (`type`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='数据字典类型表';"#,
    r#"CREATE TABLE `sys_dictionary_detail` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '字典项主键',
  `dictionary_id` bigint unsigned NOT NULL COMMENT '所属字典类型 ID（sys_dictionary.id）',
  `label` varchar(255) NOT NULL DEFAULT '' COMMENT '字典项显示文本',
  `value` varchar(255) NOT NULL DEFAULT '' COMMENT '字典值（同类型下活记录唯一）',
  `extend` varchar(255) NOT NULL DEFAULT '' COMMENT '扩展字段（JSON 字符串，业务自定义）',
  `sort` int NOT NULL DEFAULT '0' COMMENT '排序值；数值越小越靠前',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  KEY `idx_sys_dictionary_detail_dictionary_id` (`dictionary_id`,`sort`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='数据字典项表';"#,
    r#"CREATE TABLE `sys_file` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '文件记录主键',
  `name` varchar(255) NOT NULL COMMENT '原始文件名（含扩展名，下载时用于 Content-Disposition）',
  `stored_name` varchar(255) NOT NULL COMMENT '磁盘存储名：<uuid>.<ext>（唯一，列表按此定位文件）',
  `ext` varchar(20) NOT NULL DEFAULT '' COMMENT '小写扩展名（白名单校验依据）',
  `mime` varchar(100) NOT NULL DEFAULT '' COMMENT 'Content-Type',
  `size` bigint unsigned NOT NULL DEFAULT '0' COMMENT '字节数',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '上传人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id；无更新端点，写入恒等于上传人）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_file_stored_name` (`stored_name`),
  KEY `idx_sys_file_created_at` (`created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='文件上传记录（本地磁盘存储）';"#,
    r#"CREATE TABLE `sys_job` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '任务主键',
  `job_name` varchar(64) NOT NULL COMMENT '任务名称（唯一，含软删占位）',
  `cron_expr` varchar(64) NOT NULL COMMENT 'cron 表达式（6 段秒级：秒 分 时 日 月 周）',
  `handler_name` varchar(64) NOT NULL COMMENT '任务处理器名（内置注册表键）',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID（sys_user.id；0 表示种子/系统写入）',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID（sys_user.id）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_job_job_name` (`job_name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='定时任务表';"#,
    r#"CREATE TABLE `sys_job_log` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '执行日志主键',
  `job_id` bigint unsigned NOT NULL COMMENT '任务 ID（sys_job.id；任务删除后日志保留）',
  `job_name` varchar(64) NOT NULL COMMENT '任务名称（冗余存，主任务删除后仍可读）',
  `status` tinyint NOT NULL DEFAULT '0' COMMENT '执行结果：1=成功，0=失败（含超时/panic）',
  `error_msg` text NOT NULL DEFAULT (_utf8mb4'') COMMENT '失败原因（UTF-8 边界截断 2KB）；成功为空字符串',
  `duration_ms` int unsigned NOT NULL DEFAULT '0' COMMENT '本次耗时（毫秒）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  KEY `idx_sys_job_log_job_id_created_at` (`job_id`,`created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='定时任务执行日志表';"#,
    r#"CREATE TABLE `sys_login_log` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '日志主键',
  `user_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '登录用户 ID（sys_user.id；失败为 0）',
  `username` varchar(64) NOT NULL COMMENT '本次登录尝试的用户名（超长截断 64 字符）',
  `ip` varchar(64) NOT NULL DEFAULT '' COMMENT '来源 IP',
  `agent` varchar(255) NOT NULL DEFAULT '' COMMENT 'User-Agent（超长截断 255 字符）',
  `status` tinyint NOT NULL DEFAULT '0' COMMENT '结果：1=成功，0=失败',
  `msg` varchar(255) NOT NULL DEFAULT '' COMMENT '结果说明（失败原因或成功提示，如「密码错误」「登录成功」）',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  KEY `idx_sys_login_log_created_at` (`created_at`),
  KEY `idx_sys_login_log_username` (`username`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='登录日志（认证模块自动落库，成功/失败均记录）';"#,
    r#"CREATE TABLE `sys_menu` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '菜单/按钮主键',
  `parent_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '上级菜单 ID；0 表示顶级节点',
  `path` varchar(255) NOT NULL DEFAULT '' COMMENT '前端路由路径；目录或菜单使用',
  `name` varchar(100) NOT NULL DEFAULT '' COMMENT '前端路由名；业务语义上全局唯一',
  `component` varchar(255) NOT NULL DEFAULT '' COMMENT 'Vben 动态路由组件路径；必须能被 views glob 匹配',
  `title` varchar(100) NOT NULL DEFAULT '' COMMENT '菜单标题',
  `icon` varchar(100) NOT NULL DEFAULT '' COMMENT '菜单图标名称；空字符串表示无图标',
  `sort` int NOT NULL DEFAULT '0' COMMENT '排序值；数值越小越靠前',
  `keep_alive` tinyint NOT NULL DEFAULT '0' COMMENT '页面缓存：1=开启 keep-alive，0=关闭',
  `hidden` tinyint NOT NULL DEFAULT '0' COMMENT '菜单可见性：1=隐藏，0=显示',
  `menu_type` tinyint NOT NULL DEFAULT '2' COMMENT '类型：1=目录，2=菜单，3=按钮',
  `permission` varchar(100) NOT NULL DEFAULT '' COMMENT '按钮权限码；前后端授权同源的唯一事实来源',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_menu_name` (`name`),
  KEY `idx_sys_menu_parent_id` (`parent_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='菜单与按钮权限表';"#,
    r#"CREATE TABLE `sys_operation_log` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '日志主键',
  `user_id` bigint unsigned NOT NULL COMMENT '操作人 ID（sys_user.id；未鉴权为 0）',
  `ip` varchar(64) NOT NULL DEFAULT '' COMMENT '来源 IP',
  `method` varchar(16) NOT NULL DEFAULT '' COMMENT 'HTTP 请求方法（大写，如 GET/POST）',
  `path` varchar(255) NOT NULL COMMENT '请求路径',
  `status` int NOT NULL DEFAULT '0' COMMENT 'HTTP 状态码',
  `latency` bigint NOT NULL DEFAULT '0' COMMENT '请求耗时（毫秒）',
  `agent` varchar(255) NOT NULL DEFAULT '' COMMENT 'User-Agent（超长截断 255 字符）',
  `body` text NOT NULL COMMENT '请求体（敏感字段脱敏、UTF-8 边界截断 4KB 后）',
  `resp` text NOT NULL COMMENT '响应体（敏感字段脱敏、UTF-8 边界截断 4KB 后）',
  `error_message` varchar(500) NOT NULL DEFAULT '' COMMENT '业务失败提示或传输层错误摘要；成功为空字符串',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`),
  KEY `idx_sys_operation_log_created_at` (`created_at`),
  KEY `idx_sys_operation_log_user_id` (`user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='操作日志（已登录业务请求中间件自动落库，脱敏截断后存储）';"#,
    r#"CREATE TABLE `sys_position` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `position_code` varchar(64) NOT NULL COMMENT '职位编码（全局唯一，含软删占位）',
  `position_name` varchar(64) NOT NULL COMMENT '职位名称',
  `sort` int NOT NULL DEFAULT '0' COMMENT '排序值，越小越靠前',
  `status` tinyint(1) NOT NULL DEFAULT '1' COMMENT '状态：1 启用 / 0 停用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  `deleted_at` datetime DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_position_code` (`position_code`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='职位（主数据，用户多值挂载）';"#,
    r#"CREATE TABLE `sys_role` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '角色主键',
  `role_name` varchar(50) NOT NULL COMMENT '角色显示名称',
  `role_key` varchar(50) NOT NULL COMMENT '角色唯一标识；JWT roles 使用该值，super 表示超级管理员',
  `sort` int NOT NULL DEFAULT '0' COMMENT '排序值；数值越小越靠前',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `remark` varchar(255) NOT NULL DEFAULT '' COMMENT '备注说明',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_role_role_key` (`role_key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='角色表';"#,
    r#"CREATE TABLE `sys_role_api` (
  `role_id` bigint unsigned NOT NULL COMMENT '角色 ID，联合主键，关联 sys_role.id',
  `api_id` bigint unsigned NOT NULL COMMENT '接口权限 ID，联合主键，关联 sys_api.id',
  PRIMARY KEY (`role_id`,`api_id`),
  KEY `idx_sys_role_api_api_id` (`api_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='角色接口关联表';"#,
    r#"CREATE TABLE `sys_role_menu` (
  `role_id` bigint unsigned NOT NULL COMMENT '角色 ID，联合主键，关联 sys_role.id',
  `menu_id` bigint unsigned NOT NULL COMMENT '菜单/按钮 ID，联合主键，关联 sys_menu.id',
  PRIMARY KEY (`role_id`,`menu_id`),
  KEY `idx_sys_role_menu_menu_id` (`menu_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='角色菜单关联表';"#,
    r#"CREATE TABLE `sys_site_config` (
  `id` bigint unsigned NOT NULL COMMENT '站点配置主键（恒为 1）',
  `name` varchar(64) NOT NULL DEFAULT '' COMMENT '站点名称',
  `logo` varchar(255) NOT NULL DEFAULT '' COMMENT 'logo 图片 URL',
  `ico` varchar(255) NOT NULL DEFAULT '' COMMENT '浏览器 tab 图标 URL',
  `watermark_text` varchar(64) NOT NULL DEFAULT '' COMMENT '水印文字',
  `watermark_enable` tinyint(1) NOT NULL DEFAULT '0' COMMENT '水印开关 0/1',
  `watermark_type` varchar(16) NOT NULL DEFAULT 'text' COMMENT '水印类型：text / pic',
  `watermark_pic` varchar(255) NOT NULL DEFAULT '' COMMENT '水印图片 URL',
  `mode` varchar(8) NOT NULL DEFAULT 'white' COMMENT '主题白黑：white / black',
  `side_mode` varchar(8) NOT NULL DEFAULT 'dark' COMMENT '侧边栏模式：dark / light / head',
  `color` varchar(16) NOT NULL DEFAULT '#409EFF' COMMENT '主题色',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='网站设置（单行 id=1）';"#,
    r#"CREATE TABLE `sys_user` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '用户主键',
  `username` varchar(50) NOT NULL COMMENT '登录用户名，全局唯一',
  `password` varchar(100) NOT NULL COMMENT 'Argon2id 密码哈希，存储 PHC 格式字符串',
  `nickname` varchar(50) NOT NULL DEFAULT '' COMMENT '用户昵称，界面显示名称',
  `phone` varchar(20) NOT NULL DEFAULT '' COMMENT '手机号',
  `email` varchar(100) NOT NULL DEFAULT '' COMMENT '邮箱地址',
  `avatar` varchar(255) NOT NULL DEFAULT '' COMMENT '头像 URL，空字符串表示未设置',
  `status` tinyint NOT NULL DEFAULT '1' COMMENT '状态：1=启用，0=禁用',
  `created_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  `updated_at` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT '更新时间；MySQL 自动刷新',
  `deleted_at` datetime DEFAULT NULL COMMENT '软删除时间；NULL 表示未删除',
  `emp_no` varchar(50) NOT NULL DEFAULT '' COMMENT '工号；员工编号，空字符串表示未设置',
  `created_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建人 ID',
  `updated_by` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新人 ID',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_sys_user_username` (`username`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='用户表';"#,
    r#"CREATE TABLE `sys_user_dept` (
  `user_id` bigint unsigned NOT NULL COMMENT '用户 ID（sys_user.id）',
  `dept_id` bigint unsigned NOT NULL COMMENT '部门 ID（sys_dept.id）',
  `is_primary` tinyint(1) NOT NULL DEFAULT '0' COMMENT '主部门：1 是 / 0 否（每用户至多一个 1）',
  `is_leader` tinyint(1) NOT NULL DEFAULT '0' COMMENT '本部门负责人位：1 是 / 0 否（数据权限直控凭据）',
  `primary_owner` bigint unsigned GENERATED ALWAYS AS (if((`is_primary` = 1),`user_id`,NULL)) VIRTUAL COMMENT '主部门占位列：is_primary=1 时为 user_id，否则 NULL（配合唯一索引）',
  PRIMARY KEY (`user_id`,`dept_id`),
  UNIQUE KEY `uk_sys_user_dept_primary` (`primary_owner`),
  KEY `idx_sys_user_dept_dept_id` (`dept_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='用户-部门关联（多对多，硬删）';"#,
    r#"CREATE TABLE `sys_user_position` (
  `user_id` bigint unsigned NOT NULL COMMENT '用户 ID（sys_user.id）',
  `position_id` bigint unsigned NOT NULL COMMENT '职位 ID（sys_position.id）',
  PRIMARY KEY (`user_id`,`position_id`),
  KEY `idx_sys_user_position_position_id` (`position_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='用户-职位关联（多对多，硬删）';"#,
    r#"CREATE TABLE `sys_user_role` (
  `user_id` bigint unsigned NOT NULL COMMENT '用户 ID，联合主键，关联 sys_user.id',
  `role_id` bigint unsigned NOT NULL COMMENT '角色 ID，联合主键，关联 sys_role.id',
  PRIMARY KEY (`user_id`,`role_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='用户角色关联表';"#,
];

/// 表名清单（与 CREATE_TABLES 一一对应，down 按逆序删除）
const TABLE_NAMES: &[&str] = &[
    "sys_api",
    "sys_config",
    "sys_dept",
    "sys_dictionary",
    "sys_dictionary_detail",
    "sys_file",
    "sys_job",
    "sys_job_log",
    "sys_login_log",
    "sys_menu",
    "sys_operation_log",
    "sys_position",
    "sys_role",
    "sys_role_api",
    "sys_role_menu",
    "sys_site_config",
    "sys_user",
    "sys_user_dept",
    "sys_user_position",
    "sys_user_role",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let sys_user_exists = conn
            .query_one(Statement::from_string(
                manager.get_database_backend(),
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sys_user'",
            ))
            .await?
            .is_some();
        if sys_user_exists {
            return Ok(());
        }
        for ddl in CREATE_TABLES {
            conn.execute_unprepared(ddl).await?;
        }
        // 网站设置种子行：INSERT IGNORE 幂等（沿用原 000012）
        conn.execute_unprepared(
            "INSERT IGNORE INTO `sys_site_config` \
               (id, name, logo, ico, watermark_text, watermark_enable, watermark_type, \
                watermark_pic, mode, side_mode, color) \
             VALUES (1, 'tide-server', '', '', '', 0, 'text', '', 'white', 'dark', '#409EFF')",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for name in TABLE_NAMES.iter().rev() {
            manager
                .get_connection()
                .execute_unprepared(&format!("DROP TABLE IF EXISTS `{name}`"))
                .await?;
        }
        Ok(())
    }
}
