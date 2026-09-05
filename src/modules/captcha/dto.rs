//! 验证码 DTO。

use salvo::oapi::ToSchema;
use serde::Serialize;

/// 验证码生成响应：`image` 为裸 base64 PNG（前端拼 `data:image/png;base64,` 前缀）。
#[derive(Debug, Serialize, ToSchema)]
pub struct CaptchaGenerateResp {
    /// 验证码 id，登录时原样回传
    pub captcha_id: String,
    /// 裸 base64 PNG 字符串
    pub image: String,
}
