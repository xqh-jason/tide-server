//! 认证域 DTO：登录请求/响应（契约 §3.2）。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

/// 登录请求：`{ "username": "...", "password": "...", "captcha_id": "...", "captcha_value": "..." }`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginReq {
    /// 用户名
    pub username: String,
    /// 密码（明文传输由 HTTPS 保证，服务端只存 Argon2id 哈希）
    pub password: String,
    /// 图形验证码 id（`POST /captcha/generate` 返回，原样回传）
    pub captcha_id: String,
    /// 用户输入的验证码答案（服务端一次性消费，错误需刷新重取）
    pub captcha_value: String,
}

/// 登录响应：vben 期望 `{ token }`。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResp {
    /// JWT 访问令牌，后续请求放入 `Authorization: Bearer <token>`
    pub token: String,
}

/// 登录请求的客户端元信息（handler 从 Request 提取后传给 service）。
#[derive(Debug, Clone, Default)]
pub struct LoginMeta {
    pub ip: String,
    pub agent: String,
}
