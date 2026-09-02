use salvo::prelude::*;
use salvo::{
    Depot,
    oapi::{endpoint, extract::JsonBody},
};

use crate::modules::sys_api::dto::{CreateApiReq, UpdateApiReq};
use crate::{
    infra::state::AppState,
    modules::sys_api::dto::{ApiListReq, ApiResp},
    utils::{ApiResult, IdReq, PageResult},
};
use crate::{modules::sys_api::service as api_service, utils::ApiResponse};

/// API 权限点列表（POST + JSON body）：分页 + keyword / status / method 过滤。
#[endpoint]
pub async fn list_apis(
    depot: &mut Depot,
    req: JsonBody<ApiListReq>,
) -> ApiResult<PageResult<ApiResp>> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let data = api_service::page_apis(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 创建 API（POST + JSON body）：path + method 查重、角色授权关联落库。
#[endpoint]
pub async fn create_api(depot: &mut Depot, req: JsonBody<CreateApiReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::create_api(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 更新 API（POST + JSON body）：全量覆盖并重建角色授权关联。
#[endpoint]
pub async fn update_api(depot: &mut Depot, req: JsonBody<UpdateApiReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::update_api(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// API 详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_api(depot: &mut Depot, req: JsonBody<IdReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::get_api(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 删除 API（POST + JSON body：`{ "id": ... }`）：级联清空角色授权并软删。
#[endpoint]
pub async fn delete_api(depot: &mut Depot, req: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    api_service::delete_api(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
