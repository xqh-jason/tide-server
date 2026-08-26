use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::user::dto::{UserListReq, UsernameReq, UserResp};
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
