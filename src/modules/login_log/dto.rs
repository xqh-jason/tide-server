//! 登录日志 DTO（codegen 生成）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_login_log;
use crate::utils::PageQuery;

/// 登录日志响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginLogResp {
    pub id: u64,
    pub user_id: u64,
    pub username: String,
    pub ip: String,
    pub agent: String,
    pub status: i8,
    pub msg: String,
    pub created_at: String,
}

impl From<sys_login_log::Model> for LoginLogResp {
    fn from(m: sys_login_log::Model) -> Self {
        Self {
            id: m.id,
            user_id: m.user_id,
            username: m.username,
            ip: m.ip,
            agent: m.agent,
            status: m.status,
            msg: m.msg,
            created_at: format!("{}", m.created_at),
        }
    }
}

/// 登录日志列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginLogListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub username: Option<String>,
    pub ip: Option<String>,
    pub status: Option<i8>,
}

/// 登录日志分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct LoginLogFilter {
    pub username: Option<String>,
    pub ip: Option<String>,
    pub status: Option<i8>,
}

/// 批量删除请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteBatchReq {
    pub ids: Vec<u64>,
}
