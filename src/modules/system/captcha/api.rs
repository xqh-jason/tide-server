//! 验证码 handler。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::system::captcha::dto::CaptchaGenerateResp;
use crate::modules::system::captcha::service as captcha_service;
use crate::utils::{ApiResponse, ApiResult};

/// 生成图形验证码（POST，公开端点）。
#[endpoint]
pub async fn generate_captcha(depot: &mut Depot) -> ApiResult<CaptchaGenerateResp> {
    let state = AppState::from_depot(depot)?;
    Ok(ApiResponse::ok(captcha_service::generate_captcha(
        state.cache.as_ref(),
    )?))
}
