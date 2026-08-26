//! 用户 DTO（传输对象）：entity（Model）不直接暴露给接口，经 From 转换脱敏。

use salvo::oapi::ToSchema;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::utils::{PageQuery, PageResult};

/// 用户响应体（不含密码等敏感字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResp {
    pub id: u64,
    pub username: String,
    pub nickname: String,
    pub email: String,
    pub status: i8,
}

impl From<sys_user::Model> for UserResp {
    fn from(m: sys_user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            nickname: m.nickname,
            email: m.email,
            status: m.status,
        }
    }
}

/// 用户列表请求（JSON body 源，统一契约）。
/// 分页字段用 `#[serde(flatten)]` 内嵌 `PageQuery`（单点定义在 utils），
/// 域过滤条件只在这里声明；handler 用一个 `JsonBody<UserListReq>` 提取全部。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UserListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub keyword: Option<String>, // 用户名模糊搜索
    pub status: Option<i8>,
}

/// 按用户名查询请求（JSON body 源）：`{ "username": "..." }`。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UsernameReq {
    pub username: String,
}

/// 用户信息响应（契约 §3.2）：`{ userInfo, roles }`，vben 的 fetchUserInfo 消费。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserInfoResp {
    #[serde(rename = "userInfo")]
    pub user_info: UserResp,
    pub roles: Vec<String>,
}

impl UserInfoResp {
    pub fn from_model(user: sys_user::Model, auth: &AuthUser) -> Self {
        Self {
            user_info: UserResp::from(user),
            roles: auth.roles.clone(),
        }
    }
}

/// vben 菜单树节点（契约 §3.4 字段映射，`sys_menu → vben schema` 转换层）。
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

/// 用户域特有转换：repo 的 `(total, Vec<Model>)` → 通用分页响应。
impl From<(u64, Vec<sys_user::Model>)> for PageResult<UserResp> {
    fn from((total, items): (u64, Vec<sys_user::Model>)) -> Self {
        PageResult::new(total, items.into_iter().map(UserResp::from).collect())
    }
}
