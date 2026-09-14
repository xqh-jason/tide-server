//! 定时任务执行日志 handler（codegen 生成后裁剪：只读 + 删除 + 批量删除）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::system::job_log::dto::{DeleteBatchReq, JobLogListReq, JobLogResp};
use crate::modules::system::job_log::service as job_log_service;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 执行日志列表（POST + JSON body）。
#[endpoint]
pub async fn list_job_logs(
    depot: &mut Depot,
    body: JsonBody<JobLogListReq>,
) -> ApiResult<PageResult<JobLogResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = job_log_service::page_job_logs(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 执行日志详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_job_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<JobLogResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = job_log_service::get_job_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 删除执行日志（POST + JSON body：`{ "id": ... }`，软删）。
#[endpoint]
pub async fn delete_job_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    job_log_service::delete_job_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 批量删除执行日志（POST + JSON body：`{ "ids": [...] }`，软删，返回受影响行数）。
#[endpoint]
pub async fn delete_job_log_batch(
    depot: &mut Depot,
    body: JsonBody<DeleteBatchReq>,
) -> ApiResult<u64> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let affected = job_log_service::delete_job_log_batch(&state.db, &req.ids).await?;
    Ok(ApiResponse::ok(affected))
}
