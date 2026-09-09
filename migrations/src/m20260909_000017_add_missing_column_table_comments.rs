//! W7 迁移：为日志/字典/任务等表补齐中文表注释与字段注释。
//!
//! 背景：m20260827_000002/000003 只治理了 RBAC 7 表；后续 W5/W6 新建的日志、字典、
//! 任务、文件/配置类表未做注释治理。本迁移只补注释与收敛列定义，不改业务行为：
//! - 6 张无表注释表补表注释；
//! - 9 张表补齐缺失的列注释（`MODIFY COLUMN` 必须完整复述列定义，故重新声明
//!   类型/默认值；刻意不声明主键、唯一键或普通索引，避免重复创建/覆盖既有索引）；
//! - `sys_job_log.error_msg` 由 `TEXT NULL` 收紧为 `TEXT NOT NULL DEFAULT ('')`
//!   （调度器写路径恒给非空字符串，见 job/scheduler.rs）；
//! - `sys_file.updated_by` 注释升级为如实口径（无更新端点，写入恒等于上传人）。

use sea_orm_migration::prelude::*;

fn primary_id<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.big_unsigned()
        .not_null()
        .auto_increment()
        .comment(comment);
    def
}

fn relation_id<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.big_unsigned().not_null().comment(comment);
    def
}

fn not_null_string<T>(column: T, length: u32, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.string_len(length).not_null().comment(comment);
    def
}

fn default_string<T>(column: T, length: u32, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.string_len(length)
        .not_null()
        .default("")
        .comment(comment);
    def
}

fn default_big_unsigned<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.big_unsigned().not_null().default(0).comment(comment);
    def
}

fn default_integer<T>(column: T, value: i32, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.integer().not_null().default(value).comment(comment);
    def
}

fn default_tiny_integer<T>(column: T, value: i8, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.tiny_integer()
        .not_null()
        .default(value)
        .comment(comment);
    def
}

fn default_big_integer<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.big_integer().not_null().default(0).comment(comment);
    def
}

fn default_unsigned_integer<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.unsigned().not_null().default(0).comment(comment);
    def
}

fn text_not_null<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.text().not_null().comment(comment);
    def
}

fn created_at<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.date_time()
        .not_null()
        .default(Expr::current_timestamp())
        .comment(comment);
    def
}

fn updated_at<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.date_time()
        .not_null()
        .default(Expr::current_timestamp())
        .extra("ON UPDATE CURRENT_TIMESTAMP")
        .comment(comment);
    def
}

fn deleted_at<T>(column: T, comment: &str) -> ColumnDef
where
    T: IntoIden,
{
    let mut def = ColumnDef::new(column);
    def.date_time().null().comment(comment);
    def
}

#[derive(DeriveIden)]
enum SysOperationLog {
    Table,
    Id,
    UserId,
    Ip,
    Method,
    Path,
    Status,
    Latency,
    Agent,
    Body,
    Resp,
    ErrorMessage,
    CreatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysLoginLog {
    Table,
    Id,
    UserId,
    Username,
    Ip,
    Agent,
    Status,
    Msg,
    CreatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysDictionary {
    Table,
    Id,
    Name,
    Type,
    Status,
    Remark,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysDictionaryDetail {
    Table,
    Id,
    DictionaryId,
    Label,
    Value,
    Extend,
    Sort,
    Status,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysFile {
    Table,
    Id,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysConfig {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysSiteConfig {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysJob {
    Table,
    Id,
    JobName,
    CronExpr,
    HandlerName,
    Status,
    Remark,
    CreatedBy,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysJobLog {
    Table,
    Id,
    JobId,
    JobName,
    Status,
    DurationMs,
    CreatedAt,
    DeletedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

/// 表名和注释都是迁移内静态常量，避免动态拼接不可信输入。
const TABLE_COMMENTS: [(&str, &str); 6] = [
    (
        "sys_operation_log",
        "操作日志（已登录业务请求中间件自动落库，脱敏截断后存储）",
    ),
    (
        "sys_login_log",
        "登录日志（认证模块自动落库，成功/失败均记录）",
    ),
    ("sys_dictionary", "数据字典类型表"),
    ("sys_dictionary_detail", "数据字典项表"),
    ("sys_job", "定时任务表"),
    ("sys_job_log", "定时任务执行日志表"),
];

async fn set_table_comments(
    manager: &SchemaManager<'_>,
    comments: &[(&str, &str)],
) -> Result<(), DbErr> {
    let connection = manager.get_connection();
    for (table, comment) in comments {
        let sql = format!("ALTER TABLE `{table}` COMMENT = '{comment}'");
        connection.execute_unprepared(&sql).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1) sys_dictionary：补 8 列（created_by/updated_by 已有注释，不动）
        manager
            .alter_table(
                Table::alter()
                    .table(SysDictionary::Table)
                    .modify_column(primary_id(SysDictionary::Id, "字典类型主键"))
                    .modify_column(default_string(
                        SysDictionary::Name,
                        64,
                        "字典名称（显示名）",
                    ))
                    .modify_column(not_null_string(
                        SysDictionary::Type,
                        64,
                        "字典类型编码（全局唯一，含软删占位；SQL 列名 type 为保留字）",
                    ))
                    .modify_column(default_tiny_integer(
                        SysDictionary::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(default_string(SysDictionary::Remark, 255, "备注"))
                    .modify_column(created_at(SysDictionary::CreatedAt, "创建时间"))
                    .modify_column(updated_at(
                        SysDictionary::UpdatedAt,
                        "更新时间；MySQL 自动刷新",
                    ))
                    .modify_column(deleted_at(
                        SysDictionary::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 2) sys_dictionary_detail：补 10 列
        manager
            .alter_table(
                Table::alter()
                    .table(SysDictionaryDetail::Table)
                    .modify_column(primary_id(SysDictionaryDetail::Id, "字典项主键"))
                    .modify_column(relation_id(
                        SysDictionaryDetail::DictionaryId,
                        "所属字典类型 ID（sys_dictionary.id）",
                    ))
                    .modify_column(default_string(
                        SysDictionaryDetail::Label,
                        255,
                        "字典项显示文本",
                    ))
                    .modify_column(default_string(
                        SysDictionaryDetail::Value,
                        255,
                        "字典值（同类型下活记录唯一）",
                    ))
                    .modify_column(default_string(
                        SysDictionaryDetail::Extend,
                        255,
                        "扩展字段（JSON 字符串，业务自定义）",
                    ))
                    .modify_column(default_integer(
                        SysDictionaryDetail::Sort,
                        0,
                        "排序值；数值越小越靠前",
                    ))
                    .modify_column(default_tiny_integer(
                        SysDictionaryDetail::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(created_at(SysDictionaryDetail::CreatedAt, "创建时间"))
                    .modify_column(updated_at(
                        SysDictionaryDetail::UpdatedAt,
                        "更新时间；MySQL 自动刷新",
                    ))
                    .modify_column(deleted_at(
                        SysDictionaryDetail::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 3) sys_operation_log：13 列全补
        manager
            .alter_table(
                Table::alter()
                    .table(SysOperationLog::Table)
                    .modify_column(primary_id(SysOperationLog::Id, "日志主键"))
                    .modify_column(relation_id(
                        SysOperationLog::UserId,
                        "操作人 ID（sys_user.id；未鉴权为 0）",
                    ))
                    .modify_column(default_string(SysOperationLog::Ip, 64, "来源 IP"))
                    .modify_column(default_string(
                        SysOperationLog::Method,
                        16,
                        "HTTP 请求方法（大写，如 GET/POST）",
                    ))
                    .modify_column(not_null_string(SysOperationLog::Path, 255, "请求路径"))
                    .modify_column(default_integer(SysOperationLog::Status, 0, "HTTP 状态码"))
                    .modify_column(default_big_integer(
                        SysOperationLog::Latency,
                        "请求耗时（毫秒）",
                    ))
                    .modify_column(default_string(
                        SysOperationLog::Agent,
                        255,
                        "User-Agent（超长截断 255 字符）",
                    ))
                    .modify_column(text_not_null(
                        SysOperationLog::Body,
                        "请求体（敏感字段脱敏、UTF-8 边界截断 4KB 后）",
                    ))
                    .modify_column(text_not_null(
                        SysOperationLog::Resp,
                        "响应体（敏感字段脱敏、UTF-8 边界截断 4KB 后）",
                    ))
                    .modify_column(default_string(
                        SysOperationLog::ErrorMessage,
                        500,
                        "业务失败提示或传输层错误摘要；成功为空字符串",
                    ))
                    .modify_column(created_at(SysOperationLog::CreatedAt, "创建时间"))
                    .modify_column(deleted_at(
                        SysOperationLog::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 4) sys_login_log：9 列全补
        manager
            .alter_table(
                Table::alter()
                    .table(SysLoginLog::Table)
                    .modify_column(primary_id(SysLoginLog::Id, "日志主键"))
                    .modify_column(default_big_unsigned(
                        SysLoginLog::UserId,
                        "登录用户 ID（sys_user.id；失败为 0）",
                    ))
                    .modify_column(not_null_string(
                        SysLoginLog::Username,
                        64,
                        "本次登录尝试的用户名（超长截断 64 字符）",
                    ))
                    .modify_column(default_string(SysLoginLog::Ip, 64, "来源 IP"))
                    .modify_column(default_string(
                        SysLoginLog::Agent,
                        255,
                        "User-Agent（超长截断 255 字符）",
                    ))
                    .modify_column(default_tiny_integer(
                        SysLoginLog::Status,
                        0,
                        "结果：1=成功，0=失败",
                    ))
                    .modify_column(default_string(
                        SysLoginLog::Msg,
                        255,
                        "结果说明（失败原因或成功提示，如「密码错误」「登录成功」）",
                    ))
                    .modify_column(created_at(SysLoginLog::CreatedAt, "创建时间"))
                    .modify_column(deleted_at(
                        SysLoginLog::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 5) sys_file：补 id/created_at/updated_at/deleted_at；updated_by 注释如实化
        manager
            .alter_table(
                Table::alter()
                    .table(SysFile::Table)
                    .modify_column(primary_id(SysFile::Id, "文件记录主键"))
                    .modify_column(default_big_unsigned(
                        SysFile::UpdatedBy,
                        "更新人 ID（sys_user.id；无更新端点，写入恒等于上传人）",
                    ))
                    .modify_column(created_at(SysFile::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysFile::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(
                        SysFile::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 6) sys_config：补四列
        manager
            .alter_table(
                Table::alter()
                    .table(SysConfig::Table)
                    .modify_column(primary_id(SysConfig::Id, "参数主键"))
                    .modify_column(created_at(SysConfig::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysConfig::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(
                        SysConfig::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 7) sys_site_config：补四列（id 无自增，用 relation_id 等价声明）
        manager
            .alter_table(
                Table::alter()
                    .table(SysSiteConfig::Table)
                    .modify_column(relation_id(SysSiteConfig::Id, "站点配置主键（恒为 1）"))
                    .modify_column(created_at(SysSiteConfig::CreatedAt, "创建时间"))
                    .modify_column(updated_at(
                        SysSiteConfig::UpdatedAt,
                        "更新时间；MySQL 自动刷新",
                    ))
                    .modify_column(deleted_at(
                        SysSiteConfig::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 8) sys_job：11 列全补
        manager
            .alter_table(
                Table::alter()
                    .table(SysJob::Table)
                    .modify_column(primary_id(SysJob::Id, "任务主键"))
                    .modify_column(not_null_string(
                        SysJob::JobName,
                        64,
                        "任务名称（唯一，含软删占位）",
                    ))
                    .modify_column(not_null_string(
                        SysJob::CronExpr,
                        64,
                        "cron 表达式（6 段秒级：秒 分 时 日 月 周）",
                    ))
                    .modify_column(not_null_string(
                        SysJob::HandlerName,
                        64,
                        "任务处理器名（内置注册表键）",
                    ))
                    .modify_column(default_tiny_integer(
                        SysJob::Status,
                        1,
                        "状态：1=启用，0=禁用",
                    ))
                    .modify_column(default_string(SysJob::Remark, 255, "备注"))
                    .modify_column(default_big_unsigned(
                        SysJob::CreatedBy,
                        "创建人 ID（sys_user.id；0 表示种子/系统写入）",
                    ))
                    .modify_column(default_big_unsigned(
                        SysJob::UpdatedBy,
                        "更新人 ID（sys_user.id）",
                    ))
                    .modify_column(created_at(SysJob::CreatedAt, "创建时间"))
                    .modify_column(updated_at(SysJob::UpdatedAt, "更新时间；MySQL 自动刷新"))
                    .modify_column(deleted_at(SysJob::DeletedAt, "软删除时间；NULL 表示未删除"))
                    .to_owned(),
            )
            .await?;

        // 9) sys_job_log：error_msg 收紧为 TEXT NOT NULL DEFAULT ('')，其余 7 列补注释。
        //    MySQL 对 TEXT 只接受表达式默认值，字面量 `DEFAULT ''` 会报 1101，
        //    故 error_msg 必须走原生 SQL，不走 ColumnDef。
        let db = manager.get_connection();
        db.execute_unprepared(
            "UPDATE `sys_job_log` SET `error_msg` = '' WHERE `error_msg` IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE `sys_job_log` \
             MODIFY COLUMN `error_msg` TEXT NOT NULL DEFAULT ('') \
             COMMENT '失败原因（UTF-8 边界截断 2KB）；成功为空字符串'",
        )
        .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(SysJobLog::Table)
                    .modify_column(primary_id(SysJobLog::Id, "执行日志主键"))
                    .modify_column(relation_id(
                        SysJobLog::JobId,
                        "任务 ID（sys_job.id；任务删除后日志保留）",
                    ))
                    .modify_column(not_null_string(
                        SysJobLog::JobName,
                        64,
                        "任务名称（冗余存，主任务删除后仍可读）",
                    ))
                    .modify_column(default_tiny_integer(
                        SysJobLog::Status,
                        0,
                        "执行结果：1=成功，0=失败（含超时/panic）",
                    ))
                    .modify_column(default_unsigned_integer(
                        SysJobLog::DurationMs,
                        "本次耗时（毫秒）",
                    ))
                    .modify_column(created_at(SysJobLog::CreatedAt, "创建时间"))
                    .modify_column(deleted_at(
                        SysJobLog::DeletedAt,
                        "软删除时间；NULL 表示未删除",
                    ))
                    .to_owned(),
            )
            .await?;

        // 10) 6 张表补表注释
        set_table_comments(manager, &TABLE_COMMENTS).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 清空各表本次补的列注释（sys_file.updated_by 还原为原「更新人 ID」）
        manager
            .alter_table(
                Table::alter()
                    .table(SysDictionary::Table)
                    .modify_column(primary_id(SysDictionary::Id, ""))
                    .modify_column(default_string(SysDictionary::Name, 64, ""))
                    .modify_column(not_null_string(SysDictionary::Type, 64, ""))
                    .modify_column(default_tiny_integer(SysDictionary::Status, 1, ""))
                    .modify_column(default_string(SysDictionary::Remark, 255, ""))
                    .modify_column(created_at(SysDictionary::CreatedAt, ""))
                    .modify_column(updated_at(SysDictionary::UpdatedAt, ""))
                    .modify_column(deleted_at(SysDictionary::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysDictionaryDetail::Table)
                    .modify_column(primary_id(SysDictionaryDetail::Id, ""))
                    .modify_column(relation_id(SysDictionaryDetail::DictionaryId, ""))
                    .modify_column(default_string(SysDictionaryDetail::Label, 255, ""))
                    .modify_column(default_string(SysDictionaryDetail::Value, 255, ""))
                    .modify_column(default_string(SysDictionaryDetail::Extend, 255, ""))
                    .modify_column(default_integer(SysDictionaryDetail::Sort, 0, ""))
                    .modify_column(default_tiny_integer(SysDictionaryDetail::Status, 1, ""))
                    .modify_column(created_at(SysDictionaryDetail::CreatedAt, ""))
                    .modify_column(updated_at(SysDictionaryDetail::UpdatedAt, ""))
                    .modify_column(deleted_at(SysDictionaryDetail::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysOperationLog::Table)
                    .modify_column(primary_id(SysOperationLog::Id, ""))
                    .modify_column(relation_id(SysOperationLog::UserId, ""))
                    .modify_column(default_string(SysOperationLog::Ip, 64, ""))
                    .modify_column(default_string(SysOperationLog::Method, 16, ""))
                    .modify_column(not_null_string(SysOperationLog::Path, 255, ""))
                    .modify_column(default_integer(SysOperationLog::Status, 0, ""))
                    .modify_column(default_big_integer(SysOperationLog::Latency, ""))
                    .modify_column(default_string(SysOperationLog::Agent, 255, ""))
                    .modify_column(text_not_null(SysOperationLog::Body, ""))
                    .modify_column(text_not_null(SysOperationLog::Resp, ""))
                    .modify_column(default_string(SysOperationLog::ErrorMessage, 500, ""))
                    .modify_column(created_at(SysOperationLog::CreatedAt, ""))
                    .modify_column(deleted_at(SysOperationLog::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysLoginLog::Table)
                    .modify_column(primary_id(SysLoginLog::Id, ""))
                    .modify_column(default_big_unsigned(SysLoginLog::UserId, ""))
                    .modify_column(not_null_string(SysLoginLog::Username, 64, ""))
                    .modify_column(default_string(SysLoginLog::Ip, 64, ""))
                    .modify_column(default_string(SysLoginLog::Agent, 255, ""))
                    .modify_column(default_tiny_integer(SysLoginLog::Status, 0, ""))
                    .modify_column(default_string(SysLoginLog::Msg, 255, ""))
                    .modify_column(created_at(SysLoginLog::CreatedAt, ""))
                    .modify_column(deleted_at(SysLoginLog::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysFile::Table)
                    .modify_column(primary_id(SysFile::Id, ""))
                    .modify_column(default_big_unsigned(SysFile::UpdatedBy, "更新人 ID"))
                    .modify_column(created_at(SysFile::CreatedAt, ""))
                    .modify_column(updated_at(SysFile::UpdatedAt, ""))
                    .modify_column(deleted_at(SysFile::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysConfig::Table)
                    .modify_column(primary_id(SysConfig::Id, ""))
                    .modify_column(created_at(SysConfig::CreatedAt, ""))
                    .modify_column(updated_at(SysConfig::UpdatedAt, ""))
                    .modify_column(deleted_at(SysConfig::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysSiteConfig::Table)
                    .modify_column(relation_id(SysSiteConfig::Id, ""))
                    .modify_column(created_at(SysSiteConfig::CreatedAt, ""))
                    .modify_column(updated_at(SysSiteConfig::UpdatedAt, ""))
                    .modify_column(deleted_at(SysSiteConfig::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysJob::Table)
                    .modify_column(primary_id(SysJob::Id, ""))
                    .modify_column(not_null_string(SysJob::JobName, 64, ""))
                    .modify_column(not_null_string(SysJob::CronExpr, 64, ""))
                    .modify_column(not_null_string(SysJob::HandlerName, 64, ""))
                    .modify_column(default_tiny_integer(SysJob::Status, 1, ""))
                    .modify_column(default_string(SysJob::Remark, 255, ""))
                    .modify_column(default_big_unsigned(SysJob::CreatedBy, ""))
                    .modify_column(default_big_unsigned(SysJob::UpdatedBy, ""))
                    .modify_column(created_at(SysJob::CreatedAt, ""))
                    .modify_column(updated_at(SysJob::UpdatedAt, ""))
                    .modify_column(deleted_at(SysJob::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SysJobLog::Table)
                    .modify_column(primary_id(SysJobLog::Id, ""))
                    .modify_column(relation_id(SysJobLog::JobId, ""))
                    .modify_column(not_null_string(SysJobLog::JobName, 64, ""))
                    .modify_column(default_tiny_integer(SysJobLog::Status, 0, ""))
                    .modify_column(default_unsigned_integer(SysJobLog::DurationMs, ""))
                    .modify_column(created_at(SysJobLog::CreatedAt, ""))
                    .modify_column(deleted_at(SysJobLog::DeletedAt, ""))
                    .to_owned(),
            )
            .await?;

        // error_msg 还原 TEXT NULL（无默认、无注释）
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE `sys_job_log` MODIFY COLUMN `error_msg` TEXT NULL")
            .await?;

        // 表注释置空
        let empty_comments = TABLE_COMMENTS
            .iter()
            .map(|(table, _)| (*table, ""))
            .collect::<Vec<_>>();
        set_table_comments(manager, &empty_comments).await
    }
}
