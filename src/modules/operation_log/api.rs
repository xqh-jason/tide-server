//! 操作日志 handler（codegen 生成后裁剪：只读 + 删除 + 批量删除）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::operation_log::dto::{
    DeleteBatchReq, OperationLogDetail, OperationLogItem, OperationLogListReq,
};
use crate::modules::operation_log::service as operation_log_service;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 操作日志列表（POST + JSON body）：列表项不含 body / resp。
#[endpoint]
pub async fn list_operation_logs(
    depot: &mut Depot,
    body: JsonBody<OperationLogListReq>,
) -> ApiResult<PageResult<OperationLogItem>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = operation_log_service::page_operation_logs(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 操作日志详情（POST + JSON body：`{ "id": ... }`）：含脱敏截断后的 body / resp。
#[endpoint]
pub async fn get_operation_log(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<OperationLogDetail> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = operation_log_service::get_operation_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 删除操作日志（POST + JSON body：`{ "id": ... }`，软删）。
#[endpoint]
pub async fn delete_operation_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    operation_log_service::delete_operation_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 批量删除操作日志（POST + JSON body：`{ "ids": [...] }`，软删，返回受影响行数）。
#[endpoint]
pub async fn delete_operation_log_batch(
    depot: &mut Depot,
    body: JsonBody<DeleteBatchReq>,
) -> ApiResult<u64> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let affected = operation_log_service::delete_operation_log_batch(&state.db, &req.ids).await?;
    Ok(ApiResponse::ok(affected))
}
