//! 验证码域：生成与一次性校验（W5-5）。cache 即存储，无 repo。
//!
//! 端点：`POST /api/v1/captcha/generate`（公开路由，登录前调用）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod service;

/// 验证码端点：`POST /api/v1/captcha/generate`（公开路由，不挂 AuthRequired）。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["验证码"])
        .push(Router::with_path("generate").post(api::generate_captcha))
}
