//! 用户相关 DTO（请求/响应体）。
//! entity（Model）不直接暴露给接口，经 From 转换脱敏（如 password 不外传）。

use salvo::oapi::ToSchema;
use serde::Serialize;

use crate::entity::sys_user;

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
