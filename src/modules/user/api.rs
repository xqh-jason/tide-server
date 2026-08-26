use salvo::oapi::endpoint;
use salvo::oapi::extract::PathParam;
use salvo::prelude::*;

use crate::modules::user::dto::UserResp;
use crate::modules::user::service as user_service;
use crate::infra::state::AppState;
use crate::utils::error::AppError;
use crate::utils::response::ApiResponse;

/// 按用户名查询用户（W1 验证链路：handler → state → service → repo → db → dto → 统一响应）。
///
/// 说明：`#[endpoint]` 要求返回类型的 Ok/Err 都实现 `Writer`；`Json` 与 `AppError` 均已实现。
/// `depot.get_typed::<AppState>()` 取注入的全局状态；`PathParam` 提取器自动生成 OpenAPI 文档。
#[endpoint]
pub async fn get_by_username(
    depot: &mut Depot,
    username: PathParam<String>,
) -> Result<Json<ApiResponse<Option<UserResp>>>, AppError> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let user = user_service::get_by_username(&state.db, &username.into_inner()).await?;
    Ok(Json(ApiResponse::ok(user.map(UserResp::from))))
}
