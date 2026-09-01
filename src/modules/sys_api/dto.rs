//! API 权限点 DTO：`sys_api` CRUD 传输对象。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_api;
use crate::utils::PageQuery;

/// API 权限点响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResp {
    pub id: u64,
    pub path: String,
    pub method: String,
    pub description: String,
    pub api_group: String,
    pub status: i8,
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
        }
    }
}

/// API 列表请求：分页 + keyword（path/description/api_group 模糊）/ status / method 精确过滤。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApiListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub keyword: Option<String>,
    pub status: Option<i8>,
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
    pub path: String,
    pub method: String,
    pub description: Option<String>,
    pub api_group: Option<String>,
    pub status: Option<i8>,
    pub role_ids: Vec<u64>,
}

/// 更新 API 请求（编辑表单全量提交）：`role_ids` 全量替换角色授权（空数组即清空）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateApiReq {
    pub id: u64,
    pub path: String,
    pub method: String,
    pub description: String,
    pub api_group: String,
    pub status: i8,
    pub role_ids: Vec<u64>,
}

/// 按 id 查询 / 删除 API 请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApiIdReq {
    pub id: u64,
}
