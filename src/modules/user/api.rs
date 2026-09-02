use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::user::dto::{CreateUserReq, UserInfoResp, UserListReq, UserResp, UsernameReq};
use crate::modules::user::service as user_service;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 用户列表（POST + JSON body）。分页字段（PageQuery）与过滤字段（keyword/status）
/// 通过 `#[serde(flatten)]` 合并为单个 `UserListReq`，一个 `JsonBody` 提取器取全部。
#[endpoint]
pub async fn list_users(
    depot: &mut Depot,
    body: JsonBody<UserListReq>,
) -> ApiResult<PageResult<UserResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = user_service::page_users(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 按用户名查询用户（POST + JSON body：`{ "username": "..." }`）。
///
/// 说明：`ApiResult<T>` = `Result<ApiResponse<T>, AppError>`，Ok/Err 都实现 `Writer`，
/// 由 Salvo 自动渲染；`#[endpoint]` 据此生成 OpenAPI 文档。
#[endpoint]
pub async fn get_by_username(
    depot: &mut Depot,
    body: JsonBody<UsernameReq>,
) -> ApiResult<Option<UserResp>> {
    let state = AppState::from_depot(depot)?;
    let user = user_service::get_by_username(&state.db, &body.username).await?;
    Ok(ApiResponse::ok(user.map(UserResp::from)))
}

/// 当前登录用户信息（契约 §3.2）。认证中间件已注入 `AuthUser`。
#[endpoint]
pub async fn info(depot: &mut Depot) -> ApiResult<UserInfoResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let resp = user_service::get_user_info(&state.db, &auth).await?;
    Ok(ApiResponse::ok(resp))
}

/// 权限码数组（契约 §3.2），vben `getAccessCodes` 消费，`v-access` 判断按钮显隐。
#[endpoint]
pub async fn access_codes(depot: &mut Depot) -> ApiResult<Vec<String>> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let codes = user_service::get_access_codes(&state.db, auth.user_id).await?;
    Ok(ApiResponse::ok(codes))
}

/// 创建用户（POST + JSON body）。
#[endpoint]
pub async fn create_user(depot: &mut Depot, body: JsonBody<CreateUserReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let resp = user_service::create_user(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(resp))
}

/// 获取用户详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_user(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let resp = user_service::get_user(&state.db, req.id).await?;
    Ok(ApiResponse::ok(resp))
}
