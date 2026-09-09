//! 定时任务 handler（codegen 生成后裁剪：CRUD + 状态翻转 + 立即执行）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::dictionary::service as dict_service;
use crate::modules::job::dto::{
    CreateJobReq, JobListReq, JobResp, UpdateJobReq, UpdateJobStatusReq,
};
use crate::modules::job::service as job_service;
use crate::modules::job::validate as job_validate;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 定时任务列表（POST + JSON body）。
#[endpoint]
pub async fn list_jobs(
    depot: &mut Depot,
    body: JsonBody<JobListReq>,
) -> ApiResult<PageResult<JobResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = job_service::page_jobs(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, JobResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建定时任务（POST + JSON body）。
#[endpoint]
pub async fn create_job(depot: &mut Depot, body: JsonBody<CreateJobReq>) -> ApiResult<JobResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    job_validate::validate_create_job(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = job_service::create_job(&state.db, &state, &req, auth.user_id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 更新定时任务（POST + JSON body）。
#[endpoint]
pub async fn update_job(depot: &mut Depot, body: JsonBody<UpdateJobReq>) -> ApiResult<JobResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    job_validate::validate_update_job(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = job_service::update_job(&state.db, &state, &req, auth.user_id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 定时任务详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_job(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<JobResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = job_service::get_job(&state.db, req.id).await?;
    let items = fill_user_names(&state.db, vec![model], JobResp::from).await?;
    Ok(ApiResponse::ok(items.into_iter().next().unwrap().into()))
}

/// 删除定时任务（POST + JSON body：`{ "id": ... }`，软删并同步移除调度）。
#[endpoint]
pub async fn delete_job(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    job_service::delete_job(&state.db, &state, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 更新任务状态（POST + JSON body：`{ "id": ..., "status": 0|1 }`，同步调度器）。
#[endpoint]
pub async fn update_job_status(
    depot: &mut Depot,
    body: JsonBody<UpdateJobStatusReq>,
) -> ApiResult<JobResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    job_validate::validate_update_job_status(&req, &status_allowed).map_err(AppError::Biz)?;

    let model =
        job_service::update_job_status(&state.db, &state, req.id, req.status, auth.user_id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 立即执行一次（POST + JSON body：`{ "id": ... }`，绕过 cron 后台执行）。
#[endpoint]
pub async fn run_job_once(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    job_service::run_job_once(&state.db, &state, req.id).await?;
    Ok(ApiResponse::ok(()))
}
