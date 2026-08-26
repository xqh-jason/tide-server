//! 菜单域 DTO：`sys_menu → vben schema` 转换层（契约 §3.4 字段映射）。

use salvo::oapi::ToSchema;
use serde::Serialize;

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
