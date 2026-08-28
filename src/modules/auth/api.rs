//! 认证域 handler。

use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::bearer_token;
use crate::modules::auth::dto::{LoginReq, LoginResp};
use crate::modules::auth::service as auth_service;
use crate::utils::error::AppError;
use crate::utils::{ApiResponse, ApiResult};
use std::time::Duration;

/// 登录：校验用户名密码，签发 JWT。公开接口（不挂认证中间件）。
#[endpoint]
pub async fn login(depot: &mut Depot, body: JsonBody<LoginReq>) -> ApiResult<LoginResp> {
    let state = AppState::from_depot(depot)?;
    let resp = auth_service::login(&state.db, &state.config.jwt, body.into_inner()).await?;
    Ok(ApiResponse::ok(resp))
}

/// 登出：把当前 token 加入黑名单（认证中间件校验时优先拦截）。
/// 挂在本路由上的 `AuthRequired` 已确认 token 有效，这里直接拉黑。
#[endpoint]
pub async fn logout(depot: &mut Depot, req: &mut Request) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let token = bearer_token(req).ok_or_else(|| AppError::Biz("unauthorized".into()))?;
    // 黑名单保留时间对齐 token 生命周期，避免黑名单表无限增长
    let ttl = Duration::from_secs(state.config.jwt.ttl_seconds.max(0) as u64);
    state
        .cache
        .set(&format!("jwt:blacklist:{token}"), "1".into(), ttl);
    Ok(ApiResponse::ok(()))
}
