use salvo::oapi::endpoint;
use salvo::oapi::extract::PathParam;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::user::dto::{UserQuery, UserResp};
use crate::modules::user::service as user_service;
use crate::utils::error::AppError;
use crate::utils::{ApiResponse, ApiResult, PageQuery, PageResult};

/// 按用户名查询用户（W1 验证链路：handler → state → service → repo → db → dto → 统一响应）。
///
/// 说明：`ApiResult<T>` = `Result<ApiResponse<T>, AppError>`，Ok/Err 都实现 `Writer`，
/// 由 Salvo 自动渲染；`#[endpoint]` 据此生成 OpenAPI 文档。
#[endpoint]
pub async fn get_by_username(
    depot: &mut Depot,
    username: PathParam<String>,
) -> ApiResult<Option<UserResp>> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let user = user_service::get_by_username(&state.db, &username.into_inner()).await?;
    Ok(ApiResponse::ok(user.map(UserResp::from)))
}

/// 用户列表（分页 + 动态过滤）。分页参数（PageQuery）与过滤参数（UserQuery）
/// 是两个独立提取器，字段各自从 query 取，互不干扰；分页字段统一在 utils 定义。
#[endpoint]
pub async fn list_users(
    depot: &mut Depot,
    page: PageQuery,
    query: UserQuery,
) -> ApiResult<PageResult<UserResp>> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let (total, items) = user_service::page_users(
        &state.db,
        query.keyword,
        query.status,
        page.page_index(),
        page.page_size(),
    )
    .await?;
    Ok(ApiResponse::ok((total, items).into()))
}
