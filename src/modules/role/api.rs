use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::role::dto::{CreateRoleReq, RoleIdReq, RoleListReq, RoleResp, UpdateRoleReq};
use crate::modules::role::service as role_service;
use crate::utils::error::AppError;
use crate::utils::{ApiResponse, ApiResult, PageResult};

#[endpoint]
pub async fn list_roles(
    depot: &mut Depot,
    body: JsonBody<RoleListReq>,
) -> ApiResult<PageResult<RoleResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = role_service::page_roles(&state.db, &req).await?;

    Ok(ApiResponse::ok(data.into()))
}

#[endpoint]
pub async fn create_role(depot: &mut Depot, body: JsonBody<CreateRoleReq>) -> ApiResult<RoleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let role = role_service::create_role(&state.db, &req).await?;
    Ok(ApiResponse::ok(RoleResp::from(role)))
}

#[endpoint]
pub async fn update_role(depot: &mut Depot, body: JsonBody<UpdateRoleReq>) -> ApiResult<RoleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let role = role_service::update_role(&state.db, &req).await?;
    Ok(ApiResponse::ok(RoleResp::from(role)))
}

#[endpoint]
pub async fn get_role(depot: &mut Depot, body: JsonBody<RoleIdReq>) -> ApiResult<RoleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    if let Some(role) = role_service::find_by_id(&state.db, req.id).await? {
        Ok(ApiResponse::ok(RoleResp::from(role.clone())))
    } else {
        Err(AppError::Biz("角色不存在".to_string()))
    }
}

#[endpoint]
pub async fn delete_role(depot: &mut Depot, body: JsonBody<RoleIdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let role = role_service::find_by_id(&state.db, req.id).await?;
    if let Some(_) = role {
        role_service::delete_role_by_id(&state.db, req.id).await?;
    } else {
        return Err(AppError::Biz("角色不存在".to_string()));
    }

    Ok(ApiResponse::ok(()))
}
