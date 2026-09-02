//! 数据字典 handler（codegen 生成）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::dict::dto::{CreateDictReq, DictListReq, DictResp, UpdateDictReq};
use crate::modules::dict::service as dict_service;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 数据字典列表（POST + JSON body）。
#[endpoint]
pub async fn list_dicts(
    depot: &mut Depot,
    body: JsonBody<DictListReq>,
) -> ApiResult<PageResult<DictResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = dict_service::page_dicts(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 创建数据字典（POST + JSON body）。
#[endpoint]
pub async fn create_dict(depot: &mut Depot, body: JsonBody<CreateDictReq>) -> ApiResult<DictResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = dict_service::create_dict(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 更新数据字典（POST + JSON body）。
#[endpoint]
pub async fn update_dict(depot: &mut Depot, body: JsonBody<UpdateDictReq>) -> ApiResult<DictResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = dict_service::update_dict(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 数据字典详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_dict(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<DictResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = dict_service::get_dict(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 删除数据字典（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn delete_dict(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    dict_service::delete_dict(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
