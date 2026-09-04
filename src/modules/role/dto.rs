//! 角色 DTO（传输对象）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_role;
use crate::utils::PageQuery;

/// 角色响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct RoleResp {
    /// 角色 id
    pub id: u64,
    /// 角色名称（显示名，全局唯一）
    pub role_name: String,
    /// 角色键（编码，全局唯一；`super` 为内置超管）
    pub role_key: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 创建人 ID（`sys_user.id`；种子数据为 `null`）
    pub created_by: Option<u64>,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: Option<u64>,
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
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
        }
    }
}

/// 角色列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RoleListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 角色名 / 角色键模糊搜索关键字；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 禁用；不传查全部
    pub status: Option<i8>,
}

/// 角色分页过滤条件（repo 层入参）：过滤字段与分页参数分离。
#[derive(Debug, Clone, Default)]
pub struct RoleFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

/// 创建角色请求：`menu_ids` / `api_ids` 为 `Some` 时全量替换关联，`None` 表示不设置。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRoleReq {
    /// 角色名称（显示名，全局唯一）
    pub role_name: String,
    /// 角色键（编码，全局唯一，含软删占位）
    pub role_key: String,
    /// 排序值；缺省 0
    pub sort: Option<i32>,
    /// 状态：`1` 启用（默认）、`0` 禁用
    pub status: Option<i8>,
    /// 备注，可空
    pub remark: Option<String>,
    /// 绑定的菜单 ID 列表；`None` 不设置
    pub menu_ids: Option<Vec<u64>>,
    /// 绑定的 API 权限点 ID 列表；`None` 不设置
    pub api_ids: Option<Vec<u64>>,
}

/// 更新角色请求（编辑表单全量提交）：所有字段必填；
/// `menu_ids` / `api_ids` 全量替换关联，传空数组即清空关联。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateRoleReq {
    /// 目标角色 id
    pub id: u64,
    /// 角色名称（全局唯一，排除自身查重）
    pub role_name: String,
    /// 角色键（全局唯一，排除自身查重；不能改为内置 `super`）
    pub role_key: String,
    /// 排序值
    pub sort: i32,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 绑定的菜单 ID 列表（全量替换，空数组即清空）
    pub menu_ids: Vec<u64>,
    /// 绑定的 API 权限点 ID 列表（全量替换，空数组即清空）
    pub api_ids: Vec<u64>,
}

/// 更新角色状态请求：仅传 `id` / `status` 即可。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateRoleStatusReq {
    /// 目标角色 id（内置 `super` 不允许修改）
    pub id: u64,
    /// 目标状态：`1` 启用、`0` 禁用
    pub status: i8,
}
