//! 用户 DTO（传输对象）：entity（Model）不直接暴露给接口，经 From 转换脱敏。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::utils::PageQuery;

/// 用户响应体（不含密码等敏感字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResp {
    pub id: u64,
    pub username: String,
    /// 工号；员工编号，空字符串表示未设置。
    pub emp_no: String,
    pub nickname: String,
    pub email: String,
    pub status: i8,
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

/// 创建用户请求：`{ username, password, nickname?, phone?, email?, status?, role_ids?: [] }`
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserReq {
    pub username: String,
    pub password: String,
    pub emp_no: String,
    pub nickname: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub status: Option<i8>,
    pub role_ids: Vec<u64>, // 角色 ID列表 ，允许空
}

/// 更新用户请求。
///
/// 前端编辑表单允许只提交部分字段：`password` / `phone` / `email` / `status`
/// 缺省时 service 层沿用库中原值（不覆盖），避免缺字段在提取器阶段直接 400
/// （框架级错误默认渲染为非契约体）；`password` 空串同样表示不更新密码。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserReq {
    pub id: u64,
    pub username: String,
    #[serde(default)]
    pub password: String,
    pub emp_no: String,
    pub nickname: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub status: Option<i8>,
    pub role_ids: Vec<u64>, // 角色 ID列表 ，允许空
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前端编辑表单可只提交部分字段：缺 password/phone/email/status 不应反序列化失败
    /// （否则提取器阶段直接 400，框架级错误默认渲染为非契约体）。
    #[test]
    fn update_req_missing_optional_fields_deserializes() {
        let raw = r#"{"id":1,"username":"u","emp_no":"E","nickname":"n","role_ids":[]}"#;
        let req: UpdateUserReq = serde_json::from_str(raw).expect("缺可选字段应能正常反序列化");
        assert_eq!(req.id, 1);
        assert_eq!(req.username, "u");
        assert_eq!(
            req.password, "",
            "缺 password 应默认空串（service 视为不更新密码）"
        );
        assert_eq!(req.phone, None);
        assert_eq!(req.email, None);
        assert_eq!(req.status, None);
        assert!(req.role_ids.is_empty());
    }

    /// 显式传空串/null 也应能反序列化（前端可能显式传 null）。
    #[test]
    fn update_req_accepts_null_optional_fields() {
        let raw = r#"{"id":1,"username":"u","emp_no":"E","nickname":"n","phone":null,"email":null,"status":null,"password":"","role_ids":[]}"#;
        let req: UpdateUserReq = serde_json::from_str(raw).expect("null 可选字段应能反序列化");
        assert_eq!(req.phone, None);
        assert_eq!(req.email, None);
        assert_eq!(req.status, None);
        assert_eq!(req.password, "");
    }
}
