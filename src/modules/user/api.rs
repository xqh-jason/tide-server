use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::user::dto::{UserInfoResp, UserListReq, UserResp, UsernameReq};
use crate::modules::user::service as user_service;
use crate::utils::error::AppError;
use crate::utils::{ApiResponse, ApiResult, PageResult};

/// 按用户名查询用户（POST + JSON body：`{ "username": "..." }`）。
///
/// 说明：`ApiResult<T>` = `Result<ApiResponse<T>, AppError>`，Ok/Err 都实现 `Writer`，
/// 由 Salvo 自动渲染；`#[endpoint]` 据此生成 OpenAPI 文档。
#[endpoint]
pub async fn get_by_username(
    depot: &mut Depot,
    body: JsonBody<UsernameReq>,
) -> ApiResult<Option<UserResp>> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let user = user_service::get_by_username(&state.db, &body.username).await?;
    Ok(ApiResponse::ok(user.map(UserResp::from)))
}

/// 用户列表（POST + JSON body）。分页字段（PageQuery）与过滤字段（keyword/status）
/// 通过 `#[serde(flatten)]` 合并为单个 `UserListReq`，一个 `JsonBody` 提取器取全部。
#[endpoint]
pub async fn list_users(
    depot: &mut Depot,
    body: JsonBody<UserListReq>,
) -> ApiResult<PageResult<UserResp>> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let req = body.into_inner();
    let (total, items) = user_service::page_users(
        &state.db,
        req.keyword,
        req.status,
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(ApiResponse::ok((total, items).into()))
}

/// 当前登录用户信息（契约 §3.2）。认证中间件已注入 `AuthUser`。
#[endpoint]
pub async fn info(depot: &mut Depot) -> ApiResult<UserInfoResp> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let auth = depot
        .get_typed::<AuthUser>()
        .map_err(|_| AppError::Biz("unauthorized".into()))?;
    let resp = user_service::get_user_info(&state.db, &auth).await?;
    Ok(ApiResponse::ok(resp))
}

/// 权限码数组（契约 §3.2），vben `getAccessCodes` 消费，`v-access` 判断按钮显隐。
#[endpoint]
pub async fn access_codes(depot: &mut Depot) -> ApiResult<Vec<String>> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let auth = depot
        .get_typed::<AuthUser>()
        .map_err(|_| AppError::Biz("unauthorized".into()))?;
    let codes = user_service::get_access_codes(&state.db, &auth.roles).await?;
    Ok(ApiResponse::ok(codes))
}
