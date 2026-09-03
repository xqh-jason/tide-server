//! 操作日志 DTO（codegen 生成）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_operation_log;
use crate::utils::PageQuery;

/// 操作日志列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct OperationLogListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub user_id: Option<u64>,
    pub status: Option<i32>,
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
pub struct OperationLogItem {
    pub id: u64,
    pub user_id: u64,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency: i64,
    pub agent: String,
    pub error_message: String,
    pub created_at: String, // format!("{}", m.created_at)
}
/// 操作日志详情响应：含脱敏截断后的 body / resp。
#[derive(Debug, Serialize, ToSchema)]
pub struct OperationLogDetail {
    pub id: u64,
    pub user_id: u64,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency: i64,
    pub agent: String,
    pub error_message: String,
    pub created_at: String, // format!("{}", m.created_at)
    pub body: String,
    pub resp: String,
}
/// 批量删除请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteBatchReq {
    pub ids: Vec<u64>,
}

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
            created_at: format!("{}", m.created_at),
        }
    }
}

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
            created_at: format!("{}", m.created_at),
            body: m.body,
            resp: m.resp,
        }
    }
}
