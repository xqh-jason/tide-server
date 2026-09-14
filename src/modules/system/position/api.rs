//! 职位 handler。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：list → create → update → get → delete。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::position::dto::{
    CreatePositionReq, PositionListReq, PositionResp, UpdatePositionReq,
};
use crate::modules::system::position::service as position_service;
use crate::modules::system::position::validate as position_validate;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 职位列表（POST + JSON body）。
#[endpoint]
pub async fn list_positions(
    depot: &mut Depot,
    body: JsonBody<PositionListReq>,
) -> ApiResult<PageResult<PositionResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = position_service::page_positions(&state.db, &req).await?;
    // 填充创建人和更新人显示名
    let items = fill_user_names(&state.db, data.items, PositionResp::from).await?;
    let page = crate::utils::PageResult::new(data.total, data.total_pages, items);
    Ok(ApiResponse::ok(page))
}

/// 创建职位（POST + JSON body）。
#[endpoint]
pub async fn create_position(
    depot: &mut Depot,
    body: JsonBody<CreatePositionReq>,
) -> ApiResult<PositionResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    // 值域校验（handler 层）：status 允许值从数据字典预取
    let status_allowed =
        crate::modules::system::dictionary::service::enabled_int_values(&state.db, "status")
            .await?;
    position_validate::validate_create_position(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = position_service::create_position(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], PositionResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新职位（POST + JSON body）。
#[endpoint]
pub async fn update_position(
    depot: &mut Depot,
    body: JsonBody<UpdatePositionReq>,
) -> ApiResult<PositionResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed =
        crate::modules::system::dictionary::service::enabled_int_values(&state.db, "status")
            .await?;
    position_validate::validate_update_position(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = position_service::update_position(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], PositionResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 职位详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_position(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<PositionResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = position_service::get_position(&state.db, req.id).await?;
    let resp = fill_user_names(&state.db, vec![model], PositionResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除职位（POST + JSON body：`{ "id": ... }`）；有用户挂载引用时拒绝。
#[endpoint]
pub async fn delete_position(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;
    position_service::delete_position(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}
