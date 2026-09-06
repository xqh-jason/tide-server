//! 操作日志 DTO（codegen 生成）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_operation_log;
use crate::utils::PageQuery;

/// 操作日志列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 操作用户 id 精确过滤；不传查全部
    pub user_id: Option<u64>,
    /// HTTP 响应状态码精确过滤（如 `200` / `500`）；不传查全部
    pub status: Option<i32>,
    /// 请求路径模糊搜索；不传查全部
    pub keyword: Option<String>,
}

/// 操作日志分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct OperationLogFilter {
    pub user_id: Option<u64>,
    pub status: Option<i32>,
    pub keyword: Option<String>,
}

/// 操作日志列表项响应：不含 body / resp。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogItem {
    /// 日志 id
    pub id: u64,
    /// 操作用户 id
    pub user_id: u64,
    /// 客户端 IP
    pub ip: String,
    /// HTTP 方法（GET / POST 等）
    pub method: String,
    /// 请求路径
    pub path: String,
    /// HTTP 响应状态码
    pub status: i32,
    /// 耗时（毫秒）
    pub latency: i64,
    /// 浏览器 User-Agent
    pub agent: String,
    /// 失败时的错误信息；成功为空字符串
    pub error_message: String,
    /// 记录时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
}
/// 操作日志详情响应：含脱敏截断后的 body / resp。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogDetail {
    /// 日志 id
    pub id: u64,
    /// 操作用户 id
    pub user_id: u64,
    /// 操作用户名称
    pub user_name: String,
    /// 客户端 IP
    pub ip: String,
    /// HTTP 方法（GET / POST 等）
    pub method: String,
    /// 请求路径
    pub path: String,
    /// HTTP 响应状态码
    pub status: i32,
    /// 耗时（毫秒）
    pub latency: i64,
    /// 浏览器 User-Agent
    pub agent: String,
    /// 失败时的错误信息；成功为空字符串
    pub error_message: String,
    /// 记录时间
    pub created_at: String,
    /// 请求体（脱敏截断后）
    pub body: String,
    /// 响应体（截断后）
    pub resp: String,
}
/// 批量删除请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBatchReq {
    /// 要删除的记录 id 列表（软删；不存在的 id 自动忽略）
    pub ids: Vec<u64>,
}

/// `sys_operation_log::Model` → `OperationLogItem` 字段搬运。
impl From<sys_operation_log::Model> for OperationLogItem {
    fn from(m: sys_operation_log::Model) -> Self {
        Self {
            id: m.id,
            user_id: m.user_id,
            ip: m.ip,
            method: m.method,
            path: m.path,
            status: m.status,
            latency: m.latency,
            agent: m.agent,
            error_message: m.error_message,
            created_at: crate::utils::serde_format::format_datetime(m.created_at),
            updated_at: crate::utils::serde_format::format_datetime(m.updated_at),
        }
    }
}

/// `sys_operation_log::Model` → `OperationLogDetail` 字段搬运。
impl From<sys_operation_log::Model> for OperationLogDetail {
    fn from(m: sys_operation_log::Model) -> Self {
        Self {
            id: m.id,
            user_id: m.user_id,
            ip: m.ip,
            method: m.method,
            path: m.path,
            status: m.status,
            latency: m.latency,
            agent: m.agent,
            error_message: m.error_message,
            created_at: crate::utils::serde_format::format_datetime(m.created_at),
            body: m.body,
            resp: m.resp,
            user_name: String::new(),
        }
    }
}
