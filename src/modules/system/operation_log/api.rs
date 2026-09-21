//! 操作日志 handler（codegen 生成后裁剪：只读 + 删除 + 批量删除）。

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::system::operation_log::dto::{
    DeleteBatchReq, OperationLogDetail, OperationLogItem, OperationLogListReq,
};
use crate::modules::system::operation_log::service as operation_log_service;
use crate::modules::system::user::service as user_service;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
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
    let items = fill_user_names(&state.db, data.items, OperationLogItem::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
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
    let user = user_service::get_user(&state.db, model.user_id).await?;
    let mut res: OperationLogDetail = model.into();
    res.user_name = user.username;
    Ok(ApiResponse::ok(res))
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
