//! 操作日志 handler（codegen 生成）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::operation_log::dto::{CreateOperationLogReq, OperationLogListReq, OperationLogResp, UpdateOperationLogReq};
use crate::modules::operation_log::service as operation_log_service;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};
use crate::utils::request::JsonBody;

/// 操作日志列表（POST + JSON body）。
#[endpoint]
pub async fn list_operation_logs(
    depot: &mut Depot,
    body: JsonBody<OperationLogListReq>,
) -> ApiResult<PageResult<OperationLogResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = operation_log_service::page_operation_logs(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 创建操作日志（POST + JSON body）。
#[endpoint]
pub async fn create_operation_log(depot: &mut Depot, body: JsonBody<CreateOperationLogReq>) -> ApiResult<OperationLogResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = operation_log_service::create_operation_log(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 更新操作日志（POST + JSON body）。
#[endpoint]
pub async fn update_operation_log(depot: &mut Depot, body: JsonBody<UpdateOperationLogReq>) -> ApiResult<OperationLogResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = operation_log_service::update_operation_log(&state.db, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 操作日志详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_operation_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<OperationLogResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = operation_log_service::get_operation_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 删除操作日志（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn delete_operation_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    operation_log_service::delete_operation_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
