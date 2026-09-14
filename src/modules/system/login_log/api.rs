//! 登录日志 handler（codegen 生成后裁剪：只读 + 删除 + 批量删除）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::system::login_log::dto::{DeleteBatchReq, LoginLogListReq, LoginLogResp};
use crate::modules::system::login_log::service as login_log_service;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 登录日志列表（POST + JSON body）。
#[endpoint]
pub async fn list_login_logs(
    depot: &mut Depot,
    body: JsonBody<LoginLogListReq>,
) -> ApiResult<PageResult<LoginLogResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = login_log_service::page_login_logs(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 登录日志详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_login_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<LoginLogResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = login_log_service::get_login_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 删除登录日志（POST + JSON body：`{ "id": ... }`，软删）。
#[endpoint]
pub async fn delete_login_log(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    login_log_service::delete_login_log(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 批量删除登录日志（POST + JSON body：`{ "ids": [...] }`，软删，返回受影响行数）。
#[endpoint]
pub async fn delete_login_log_batch(
    depot: &mut Depot,
    body: JsonBody<DeleteBatchReq>,
) -> ApiResult<u64> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let affected = login_log_service::delete_login_log_batch(&state.db, &req.ids).await?;
    Ok(ApiResponse::ok(affected))
}
