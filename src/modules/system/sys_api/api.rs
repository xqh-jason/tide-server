use salvo::prelude::*;

use crate::middleware::auth::AuthUser;
use crate::modules::system::dictionary::service as dict_service;
use crate::modules::system::sys_api::dto::{CreateApiReq, UpdateApiReq};
use crate::modules::system::sys_api::validate as api_validate;
use crate::utils::error::AppError;
use crate::{
    infra::state::AppState,
    modules::system::sys_api::dto::{ApiListReq, ApiResp},
    utils::{ApiResult, IdReq, PageResult},
};
use crate::{
    modules::system::sys_api::service as api_service,
    utils::{ApiResponse, request::JsonBody, user_ref::fill_user_names},
};

/// API 权限点列表（POST + JSON body）：分页 + keyword / status / method 过滤。
#[endpoint]
pub async fn list_apis(
    depot: &mut Depot,
    req: JsonBody<ApiListReq>,
) -> ApiResult<PageResult<ApiResp>> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let data = api_service::page_apis(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, ApiResp::from).await?;

    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建 API（POST + JSON body）：path + method 查重、角色授权关联落库。
/// 值域校验在拿到 `req` 后显式调用 `validate_create_api`（见同模块 validate.rs）。
#[endpoint]
pub async fn create_api(depot: &mut Depot, req: JsonBody<CreateApiReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    api_validate::validate_create_api(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = api_service::create_api(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], ApiResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新 API（POST + JSON body）：全量覆盖并重建角色授权关联。
#[endpoint]
pub async fn update_api(depot: &mut Depot, req: JsonBody<UpdateApiReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    api_validate::validate_update_api(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = api_service::update_api(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], ApiResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// API 详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_api(depot: &mut Depot, req: JsonBody<IdReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::get_api(&state.db, req.id).await?;
    let resp = fill_user_names(&state.db, vec![model], ApiResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除 API（POST + JSON body：`{ "id": ... }`）：级联清空角色授权并软删。
#[endpoint]
pub async fn delete_api(depot: &mut Depot, req: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    api_service::delete_api(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

#[endpoint]
pub async fn list_all_apis(depot: &mut Depot) -> ApiResult<Vec<ApiResp>> {
    let state = AppState::from_depot(depot)?;
    let apis = api_service::get_all_apis(&state.db).await?;
    let resp = fill_user_names(&state.db, apis, ApiResp::from).await?;
    Ok(ApiResponse::ok(resp))
}
