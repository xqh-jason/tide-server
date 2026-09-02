use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::role::dto::{
    CreateRoleReq, RoleListReq, RoleResp, UpdateRoleReq, UpdateRoleStatusReq,
};
use crate::modules::role::service as role_service;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 角色列表（POST + JSON body）：分页 + keyword / status 过滤，排除软删除。
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

/// 创建角色（POST + JSON body）：role_key / role_name 查重（含软删占位），
/// 并全量设置菜单 / API 关联。
#[endpoint]
pub async fn create_role(depot: &mut Depot, body: JsonBody<CreateRoleReq>) -> ApiResult<RoleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let role = role_service::create_role(&state.db, &req).await?;
    Ok(ApiResponse::ok(RoleResp::from(role)))
}

/// 更新角色（POST + JSON body）：编辑表单全量提交，键查重排除自身，事务全量重建关联。
#[endpoint]
pub async fn update_role(depot: &mut Depot, body: JsonBody<UpdateRoleReq>) -> ApiResult<RoleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let role = role_service::update_role(&state.db, &req).await?;
    Ok(ApiResponse::ok(RoleResp::from(role)))
}

/// 更新角色状态（POST + JSON body：`{ "id": ... }`）：判存在后更新状态，不存在返回业务错误。
#[endpoint]
pub async fn update_role_status(
    depot: &mut Depot,
    body: JsonBody<UpdateRoleStatusReq>,
) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    role_service::update_role_status(&state.db, &req).await?;
    Ok(ApiResponse::ok(()))
}

/// 角色详情（POST + JSON body：`{ "id": ... }`）：按 id 查询单个角色，不存在返回业务错误。
#[endpoint]
pub async fn get_role(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<RoleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let role = role_service::get_role(&state.db, req.id).await?;
    Ok(ApiResponse::ok(RoleResp::from(role)))
}

/// 删除角色（POST + JSON body：`{ "id": ... }`）：判存在后软删并物理清空菜单 / API 关联。
#[endpoint]
pub async fn delete_role(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    role_service::delete_role(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
