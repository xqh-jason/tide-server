//! 角色 DTO（传输对象）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_role;
use crate::utils::PageQuery;

/// 角色响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct RoleResp {
    pub id: u64,
    pub role_name: String,
    pub role_key: String,
    pub sort: i32,
    pub status: i8,
    pub remark: String,
}

impl From<sys_role::Model> for RoleResp {
    fn from(m: sys_role::Model) -> Self {
        Self {
            id: m.id,
            role_name: m.role_name,
            role_key: m.role_key,
            sort: m.sort,
            status: m.status,
            remark: m.remark,
        }
    }
}

/// 角色列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RoleListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub keyword: Option<String>, // 角色名 / 角色键模糊搜索
    pub status: Option<i8>,
}

/// 创建角色请求：`menu_ids` / `api_ids` 为 `Some` 时全量替换关联，`None` 表示不设置。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRoleReq {
    pub role_name: String,
    pub role_key: String,
    pub sort: Option<i32>,
    pub status: Option<i8>,
    pub remark: Option<String>,
    pub menu_ids: Option<Vec<u64>>,
    pub api_ids: Option<Vec<u64>>,
}

/// 更新角色请求（编辑表单全量提交）：所有字段必填；
/// `menu_ids` / `api_ids` 全量替换关联，传空数组即清空关联。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateRoleReq {
    pub id: u64,
    pub role_name: String,
    pub role_key: String,
    pub sort: i32,
    pub status: i8,
    pub remark: String,
    pub menu_ids: Vec<u64>,
    pub api_ids: Vec<u64>,
}

/// 按 id 查询 / 删除角色请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RoleIdReq {
    pub id: u64,
}
