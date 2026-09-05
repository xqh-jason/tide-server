//! API 权限点 DTO：`sys_api` CRUD 传输对象。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::entity::sys_api;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

/// API 权限点响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResp {
    /// API 权限点 id
    pub id: u64,
    /// 请求路径（如 `/api/v1/user/list`）
    pub path: String,
    /// HTTP 方法（GET / POST 等）
    pub method: String,
    /// 描述
    pub description: String,
    /// 分组名（前端分组展示用）
    pub api_group: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 创建人 ID（`sys_user.id`；`0` 表示种子/系统写入）
    pub created_by: u64,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: u64,
    /// 创建人显示名（`sys_user.username`）
    pub created_by_name: String,
    /// 更新人显示名（`sys_user.username`）
    pub updated_by_name: String,
}

impl From<sys_api::Model> for ApiResp {
    fn from(m: sys_api::Model) -> Self {
        Self {
            id: m.id,
            path: m.path,
            method: m.method,
            description: m.description,
            api_group: m.api_group,
            status: m.status,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

impl UserRefNames for ApiResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// API 列表请求：分页 + keyword（path/description/api_group 模糊）/ status / method 精确过滤。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApiListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配 path / description / api_group）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 禁用；不传查全部
    pub status: Option<i8>,
    /// HTTP 方法精确过滤（如 `POST`）；不传查全部
    pub method: Option<String>,
}

/// API 分页过滤条件（repo 层入参）：过滤字段与分页参数分离。
#[derive(Debug, Clone, Default)]
pub struct ApiFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub method: Option<String>,
}

/// 创建 API 请求：`role_ids` 为空表示暂不授权给任何角色。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiReq {
    /// 请求路径（path + method 组合唯一，含软删占位）
    pub path: String,
    /// HTTP 方法（GET / POST 等）
    pub method: String,
    /// 描述，可空
    pub description: Option<String>,
    /// 分组名，可空
    pub api_group: Option<String>,
    /// 状态：`1` 启用（默认）、`0` 禁用
    pub status: Option<i8>,
    /// 授权角色 ID 列表，允许空
    pub role_ids: Vec<u64>,
}

/// 更新 API 请求（编辑表单全量提交）：`role_ids` 全量替换角色授权（空数组即清空）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateApiReq {
    /// 目标 API 权限点 id
    pub id: u64,
    /// 请求路径（path + method 组合唯一，排除自身查重）
    pub path: String,
    /// HTTP 方法
    pub method: String,
    /// 描述
    pub description: String,
    /// 分组名
    pub api_group: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 授权角色 ID 列表（全量替换，空数组即清空）
    pub role_ids: Vec<u64>,
}
