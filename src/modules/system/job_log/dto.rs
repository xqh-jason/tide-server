//! 定时任务执行日志 DTO（codegen 生成后裁剪：只读 + 删除）。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_job_log;
use crate::utils::PageQuery;

/// 定时任务执行日志响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobLogResp {
    /// 日志 id
    pub id: u64,
    /// 任务 id（主任务删除后日志保留）
    pub job_id: u64,
    /// 任务名称（冗余存）
    pub job_name: String,
    /// 执行结果：`1` 成功、`0` 失败（含超时）
    pub status: i8,
    /// 失败原因（截断 2KB）
    pub error_msg: String,
    /// 本次耗时（毫秒）
    pub duration_ms: u32,
    /// 开始时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
}

impl From<sys_job_log::Model> for JobLogResp {
    fn from(m: sys_job_log::Model) -> Self {
        Self {
            id: m.id,
            job_id: m.job_id,
            job_name: m.job_name,
            status: m.status,
            error_msg: m.error_msg,
            duration_ms: m.duration_ms,
            created_at: crate::utils::serde_format::format_datetime(m.created_at),
        }
    }
}

/// 执行日志列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobLogListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 任务 ID 精确过滤；不传查全部
    pub job_id: Option<u64>,
    /// 执行结果精确过滤：`1` 成功、`0` 失败；不传查全部
    pub status: Option<i8>,
}

/// 执行日志分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct JobLogFilter {
    pub job_id: Option<u64>,
    pub status: Option<i8>,
}

/// 批量删除请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBatchReq {
    /// 要删除的记录 id 列表（软删；不存在的 id 自动忽略）
    pub ids: Vec<u64>,
}
