//! 定时任务 DTO（codegen 生成后裁剪）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_job;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;
use std::collections::HashMap;

/// 定时任务响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobResp {
    /// 任务 id
    pub id: u64,
    /// 任务名称（唯一含软删占位）
    pub job_name: String,
    /// cron 表达式（6 段秒级：秒 分 时 日 月 周）
    pub cron_expr: String,
    /// 任务处理器名（内置注册表键）
    pub handler_name: String,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 创建人 ID（`0` 表示种子/系统写入）
    pub created_by: u64,
    /// 更新人 ID
    pub updated_by: u64,
    /// 创建人显示名（查不到为空串，前端渲染占位符）
    pub created_by_name: String,
    /// 更新人显示名
    pub updated_by_name: String,
}

impl From<sys_job::Model> for JobResp {
    fn from(m: sys_job::Model) -> Self {
        Self {
            id: m.id,
            job_name: m.job_name,
            cron_expr: m.cron_expr,
            handler_name: m.handler_name,
            status: m.status,
            remark: m.remark,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人/更新人显示名（查不到给空串）。
impl UserRefNames for JobResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 定时任务列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 任务名称模糊搜索；不传查全部
    pub job_name: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 停用；不传查全部
    pub status: Option<i8>,
    /// 创建人 ID 精确过滤（前端用户选择器回填 id）；不传查全部
    pub created_by: Option<u64>,
    /// 更新人 ID 精确过滤；不传查全部
    pub updated_by: Option<u64>,
    /// 创建时间范围起（`yyyy-MM-dd[ HH:mm:ss]`，含边界）；不传查全部
    pub created_at_begin: Option<String>,
    /// 创建时间范围止（含边界）；不传查全部
    pub created_at_end: Option<String>,
    /// 更新时间范围起（同上格式）；不传查全部
    pub updated_at_begin: Option<String>,
    /// 更新时间范围止（含边界）；不传查全部
    pub updated_at_end: Option<String>,
}

/// 定时任务分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct JobFilter {
    pub job_name: Option<String>,
    pub status: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 创建定时任务请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateJobReq {
    /// 任务名称（唯一含软删占位）
    pub job_name: String,
    /// cron 表达式（6 段秒级）
    pub cron_expr: String,
    /// 任务处理器名（内置注册表键）
    pub handler_name: String,
    /// 状态：`1` 启用（默认）、`0` 停用
    pub status: Option<i8>,
    /// 备注，可空
    pub remark: Option<String>,
}

/// 更新定时任务请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateJobReq {
    /// 目标任务 id
    pub id: u64,
    /// 任务名称（排除自身查重）
    pub job_name: String,
    /// cron 表达式（6 段秒级）
    pub cron_expr: String,
    /// 任务处理器名（内置注册表键）
    pub handler_name: String,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 备注，可空
    pub remark: Option<String>,
}

/// 更新任务状态请求（启用 / 停用）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateJobStatusReq {
    /// 目标任务 id
    pub id: u64,
    /// `1` 启用、`0` 停用
    pub status: i8,
}
