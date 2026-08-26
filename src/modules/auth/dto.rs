//! 认证域 DTO：登录请求/响应（契约 §3.2）。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

/// 登录请求：`{ "username": "...", "password": "..." }`。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

/// 登录响应：vben 期望 `{ token }`。
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResp {
    pub token: String,
}
