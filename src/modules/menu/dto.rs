//! 菜单域 DTO：`sys_menu → vben schema` 转换层（契约 §3.4 字段映射）+ 菜单管理 CRUD。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_menu;
use crate::utils::PageQuery;

/// vben 菜单树节点。
#[derive(Debug, Serialize, ToSchema)]
pub struct VbenMenuItem {
    pub path: String,
    pub name: String,
    pub component: String,
    pub meta: VbenMenuMeta,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<VbenMenuItem>,
}

/// 菜单节点 meta（vben 使用 camelCase 字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct VbenMenuMeta {
    pub title: String,
    pub icon: String,
    pub order: i32,
    #[serde(rename = "keepAlive")]
    pub keep_alive: bool,
    #[serde(rename = "hideInMenu")]
    pub hide_in_menu: bool,
}

/// 菜单管理响应体（含菜单完整字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct MenuResp {
    pub id: u64,
    pub parent_id: u64,
    pub path: String,
    pub name: String,
    pub component: String,
    pub title: String,
    pub icon: String,
    pub sort: i32,
    pub keep_alive: i8,
    pub hidden: i8,
    pub menu_type: i8,
    pub permission: String,
    pub status: i8,
}

impl From<sys_menu::Model> for MenuResp {
    fn from(m: sys_menu::Model) -> Self {
        Self {
            id: m.id,
            parent_id: m.parent_id,
            path: m.path,
            name: m.name,
            component: m.component,
            title: m.title,
            icon: m.icon,
            sort: m.sort,
            keep_alive: m.keep_alive,
            hidden: m.hidden,
            menu_type: m.menu_type,
            permission: m.permission,
            status: m.status,
        }
    }
}

/// 菜单列表请求：分页 + keyword（title/name/path 模糊）/ status / menu_type 过滤。
#[derive(Debug, Deserialize, ToSchema)]
pub struct MenuListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub menu_type: Option<i8>,
}

/// 菜单分页过滤条件（repo 层入参）：过滤字段与分页参数分离。
#[derive(Debug, Clone, Default)]
pub struct MenuFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub menu_type: Option<i8>,
}

/// 创建菜单请求：可选字段有默认值（sort=0 / keep_alive=0 / hidden=0 / menu_type=1 / status=1）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMenuReq {
    pub parent_id: Option<u64>,
    pub path: String,
    pub name: String,
    pub component: String,
    pub title: String,
    pub icon: Option<String>,
    pub sort: Option<i32>,
    pub keep_alive: Option<i8>,
    pub hidden: Option<i8>,
    pub menu_type: Option<i8>,
    pub permission: Option<String>,
    pub status: Option<i8>,
}

/// 更新菜单请求（编辑表单全量提交）：所有字段必填，语义同角色域全量更新。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMenuReq {
    pub id: u64,
    pub parent_id: u64,
    pub path: String,
    pub name: String,
    pub component: String,
    pub title: String,
    pub icon: String,
    pub sort: i32,
    pub keep_alive: i8,
    pub hidden: i8,
    pub menu_type: i8,
    pub permission: String,
    pub status: i8,
}
