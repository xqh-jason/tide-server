//! 用户 DTO（传输对象）：entity（Model）不直接暴露给接口，经 From 转换脱敏。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::utils::PageQuery;

/// 用户响应体（不含密码等敏感字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResp {
    /// 用户 id
    pub id: u64,
    /// 用户名（登录账号，全局唯一）
    pub username: String,
    /// 工号；员工编号，空字符串表示未设置。
    pub emp_no: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 邮箱，空字符串表示未设置
    pub email: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
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

impl From<sys_user::Model> for UserResp {
    fn from(m: sys_user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            emp_no: m.emp_no,
            nickname: m.nickname,
            email: m.email,
            status: m.status,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
        }
    }
}

/// 用户列表请求（JSON body 源，统一契约）。
/// 分页字段用 `#[serde(flatten)]` 内嵌 `PageQuery`（单点定义在 utils），
/// 域过滤条件只在这里声明；handler 用一个 `JsonBody<UserListReq>` 提取全部。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UserListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 用户名模糊搜索关键字；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 禁用；不传查全部
    pub status: Option<i8>,
}

/// 用户分页过滤条件（repo 层入参）：过滤字段与分页参数分离，
/// 以后加过滤条件只改这里，repo 签名与调用点不变。
#[derive(Debug, Clone, Default)]
pub struct UserFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

/// 按用户名查询请求（JSON body 源）：`{ "username": "..." }`。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UsernameReq {
    /// 要查询的用户名
    pub username: String,
}

/// 用户信息响应（契约 §3.2）：`{ userInfo, roles }`，vben 的 fetchUserInfo 消费。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserInfoResp {
    /// 用户基本信息（不含密码）
    #[serde(rename = "userInfo")]
    pub user_info: UserResp,
    /// 当前用户角色键列表（如 `["super"]`）
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

/// 创建用户请求：`{ username, password, nickname?, phone?, email?, status?, role_ids?: [] }`
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserReq {
    /// 用户名（登录账号，全局唯一，含软删占位）
    pub username: String,
    /// 初始密码，服务端 Argon2id 哈希后落库
    pub password: String,
    /// 工号；员工编号
    pub emp_no: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 手机号，可空
    pub phone: Option<String>,
    /// 邮箱，可空
    pub email: Option<String>,
    /// 状态：`1` 启用（默认）、`0` 禁用
    pub status: Option<i8>,
    /// 角色 ID 列表，允许空（不绑角色）
    pub role_ids: Vec<u64>,
}

/// 更新用户请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserReq {
    /// 目标用户 id
    pub id: u64,
    /// 用户名（全局唯一，排除自身查重；不能与内置 admin 相同）
    pub username: String,
    /// 密码；传空串表示不更新密码（沿用原密文）
    pub password: String,
    /// 工号；员工编号
    pub emp_no: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 手机号
    pub phone: String,
    /// 邮箱
    pub email: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 角色 ID 列表（全量替换，允许空即清空角色）
    pub role_ids: Vec<u64>,
}

/// 更新用户状态
/// `{ id, status }`
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserStatusReq {
    /// 目标用户 id（内置 admin 不允许修改）
    pub id: u64,
    /// 目标状态：`1` 启用、`0` 禁用
    pub status: i8,
}
