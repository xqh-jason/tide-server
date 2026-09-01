use salvo::prelude::*;
use salvo::{
    Depot,
    oapi::{endpoint, extract::JsonBody},
};

use crate::modules::sys_api::dto::{ApiIdReq, CreateApiReq, UpdateApiReq};
use crate::{
    infra::state::AppState,
    modules::sys_api::dto::{ApiListReq, ApiResp},
    utils::{ApiResult, PageResult},
};
use crate::{modules::sys_api::service as api_service, utils::ApiResponse};

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

#[endpoint]
pub async fn create_api(depot: &mut Depot, req: JsonBody<CreateApiReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::create_api(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

#[endpoint]
pub async fn update_api(depot: &mut Depot, req: JsonBody<UpdateApiReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::update_api(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

#[endpoint]
pub async fn get_api(depot: &mut Depot, req: JsonBody<ApiIdReq>) -> ApiResult<ApiResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let model = api_service::get_api(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

#[endpoint]
pub async fn delete_api(depot: &mut Depot, req: JsonBody<ApiIdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    api_service::delete_api(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
