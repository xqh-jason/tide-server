//! 操作日志 DTO（codegen 生成）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_operation_log;
use crate::utils::PageQuery;

/// 操作日志响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct OperationLogResp {
    pub id: u64,
    pub user_id: u64,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency: i64,
    pub agent: String,
    pub body: String,
    pub resp: String,
    pub error_message: String,
}

impl From<sys_operation_log::Model> for OperationLogResp {
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
            body: m.body,
            resp: m.resp,
            error_message: m.error_message,
        }
    }
}

/// 操作日志列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct OperationLogListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub user_id: Option<u64>,
    pub status: Option<i32>,
}

/// 操作日志分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct OperationLogFilter {
    pub user_id: Option<u64>,
    pub status: Option<i32>,
}

/// 创建操作日志请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateOperationLogReq {
    pub user_id: u64,
    pub ip: Option<String>,
    pub method: Option<String>,
    pub path: String,
    pub status: Option<i32>,
    pub latency: Option<i64>,
    pub agent: Option<String>,
    pub body: Option<String>,
    pub resp: Option<String>,
    pub error_message: Option<String>,
}

/// 更新操作日志请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateOperationLogReq {
    pub id: u64,
    pub user_id: u64,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency: i64,
    pub agent: String,
    pub body: String,
    pub resp: String,
    pub error_message: String,
}

