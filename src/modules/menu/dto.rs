//! 菜单域 DTO：`sys_menu → vben schema` 转换层（契约 §3.4 字段映射）+ 菜单管理 CRUD。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::entity::sys_menu;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

/// vben 菜单树节点。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VbenMenuItem {
    /// 路由路径（如 `/system`）
    pub path: String,
    /// 路由名（唯一，vben keep-alive 依据）
    pub name: String,
    /// 组件路径（`#/views/xxx.vue`，vben glob 动态导入）
    pub component: String,
    /// 菜单元信息（标题 / 图标 / 排序等）
    pub meta: VbenMenuMeta,
    /// 子菜单；为空时序列化省略该字段
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<VbenMenuItem>,
}

/// 菜单节点 meta（vben 使用 camelCase 字段）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VbenMenuMeta {
    /// 菜单标题（侧边栏显示名）
    pub title: String,
    /// 图标名（如 `mdi:home`）
    pub icon: String,
    /// 排序值，越小越靠前
    pub order: i32,
    /// 是否 keep-alive 缓存页面
    #[serde(rename = "keepAlive")]
    pub keep_alive: bool,
    /// 是否在侧边栏隐藏
    #[serde(rename = "hideInMenu")]
    pub hide_in_menu: bool,
}

/// 菜单管理响应体（含菜单完整字段）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuResp {
    /// 菜单 id
    pub id: u64,
    /// 父菜单 id，`0` 表示顶级
    pub parent_id: u64,
    /// 路由路径（如 `/system`）
    pub path: String,
    /// 路由名（唯一，vben keep-alive 依据）
    pub name: String,
    /// 组件路径（`#/views/xxx.vue`；按钮类型为空）
    pub component: String,
    /// 菜单标题（显示名）
    pub title: String,
    /// 图标名（如 `mdi:home`）
    pub icon: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 是否 keep-alive：`1` 是、`0` 否
    pub keep_alive: i8,
    /// 是否隐藏：`1` 隐藏、`0` 显示
    pub hidden: i8,
    /// 菜单类型：`1` 目录/页面、`2` 外链、`3` 按钮
    pub menu_type: i8,
    /// 按钮权限码（如 `system:user:create`）；非按钮为空字符串
    pub permission: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 创建人 ID（`sys_user.id`；种子数据为 `null`）
    pub created_by: u64,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: u64,
    /// 创建人显示名（`sys_user.username`）
    pub created_by_name: String,
    /// 更新人显示名（`sys_user.username`）
    pub updated_by_name: String,
}

/// `sys_menu::Model` → `MenuResp` 字段搬运。
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
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充 `MenuResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for MenuResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 菜单列表请求：分页 + keyword（title/name/path 模糊）/ status / menu_type 过滤。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配 title / name / path）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 禁用；不传查全部
    pub status: Option<i8>,
    /// 菜单类型精确过滤：`1` 目录/页面、`2` 外链、`3` 按钮；不传查全部
    pub menu_type: Option<i8>,
    /// 创建人 ID 精确过滤（前端用户选择器回填 id）；不传查全部
    pub created_by: Option<u64>,
    /// 更新人 ID 精确过滤；不传查全部
    pub updated_by: Option<u64>,
    /// 创建时间范围起（yyyy-MM-dd[ HH:mm:ss]，含边界）；不传查全部
    pub created_at_begin: Option<String>,
    /// 创建时间范围止（含边界）；不传查全部
    pub created_at_end: Option<String>,
    /// 更新时间范围起（同上格式）；不传查全部
    pub updated_at_begin: Option<String>,
    /// 更新时间范围止（含边界）；不传查全部
    pub updated_at_end: Option<String>,
}

/// 菜单分页过滤条件（repo 层入参）：过滤字段与分页参数分离。
#[derive(Debug, Clone, Default)]
pub struct MenuFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub menu_type: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 创建菜单请求：可选字段有默认值（sort=0 / keep_alive=0 / hidden=0 / menu_type=1 / status=1）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMenuReq {
    /// 父菜单 id；缺省 0（顶级）
    pub parent_id: Option<u64>,
    /// 路由路径（如 `/system`）
    pub path: String,
    /// 路由名（全局唯一，含软删占位）
    pub name: String,
    /// 组件路径；非按钮必须 `#/views/xxx.vue` 格式
    #[serde(default)]
    pub component: String,
    /// 菜单标题（显示名）
    pub title: String,
    /// 图标名，可空
    pub icon: Option<String>,
    /// 排序值；缺省 0
    pub sort: Option<i32>,
    /// 是否 keep-alive；缺省 0
    pub keep_alive: Option<i8>,
    /// 是否隐藏；缺省 0
    pub hidden: Option<i8>,
    /// 菜单类型：`1` 目录/页面（默认）、`2` 外链、`3` 按钮
    pub menu_type: Option<i8>,
    /// 按钮权限码；非按钮可空
    pub permission: Option<String>,
    /// 状态：`1` 启用（默认）、`0` 禁用
    pub status: Option<i8>,
}

/// 更新菜单请求（编辑表单全量提交）：所有字段必填，语义同角色域全量更新。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMenuReq {
    /// 目标菜单 id
    pub id: u64,
    /// 父菜单 id，`0` 表示顶级
    pub parent_id: u64,
    /// 路由路径
    pub path: String,
    /// 路由名（全局唯一，排除自身查重）
    pub name: String,
    /// 组件路径；非按钮必须 `#/views/xxx.vue` 格式
    pub component: String,
    /// 菜单标题（显示名）
    pub title: String,
    /// 图标名
    pub icon: String,
    /// 排序值
    pub sort: i32,
    /// 是否 keep-alive：`1` 是、`0` 否
    pub keep_alive: i8,
    /// 是否隐藏：`1` 隐藏、`0` 显示
    pub hidden: i8,
    /// 菜单类型：`1` 目录/页面、`2` 外链、`3` 按钮
    pub menu_type: i8,
    /// 按钮权限码；非按钮为空字符串
    pub permission: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
}
