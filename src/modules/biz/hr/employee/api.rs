//! 员工档案 handler（端点函数）。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：list → create → update → get → delete。
//!
//! handler 只做三件事：取状态（`AppState`）/ 取操作人（`AuthUser`）→ 预取值域（字典）
//! → 调 validate + service → 拼显示名（`fill_user_names`）。业务规则不写在这里。
use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::biz::hr::employee::dto::{
    CreateEmployeeReq, EmployeeListReq, EmployeeResp, UpdateEmployeeReq,
};
use crate::modules::biz::hr::employee::service as employee_service;
use crate::modules::biz::hr::employee::validate as employee_validate;
use crate::modules::system::dictionary::service as dictionary_service;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 员工档案列表（POST + JSON body）。
#[endpoint]
pub async fn list_employees(
    depot: &mut Depot,
    body: JsonBody<EmployeeListReq>,
) -> ApiResult<PageResult<EmployeeResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = employee_service::page_employees(&state.db, &req).await?;
    // 填充关联账号 / 创建人 / 更新人显示名
    let items = fill_user_names(&state.db, data.items, EmployeeResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建员工档案（可选同事务创建登录账号）。
#[endpoint]
pub async fn create_employee(
    depot: &mut Depot,
    body: JsonBody<CreateEmployeeReq>,
) -> ApiResult<EmployeeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    // 值域校验（handler 层）：允许值从数据字典预取，接口不硬编码枚举
    let status_allowed =
        dictionary_service::enabled_int_values(&state.db, "employmentStatus").await?;
    let education_allowed = dictionary_service::enabled_int_values(&state.db, "education").await?;
    employee_validate::validate_create_employee(&req, &status_allowed, &education_allowed)
        .map_err(AppError::Biz)?;

    let model = employee_service::create_employee(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![model], EmployeeResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新员工档案（不修改关联账号；敏感字段空串 = 不修改）。
#[endpoint]
pub async fn update_employee(
    depot: &mut Depot,
    body: JsonBody<UpdateEmployeeReq>,
) -> ApiResult<EmployeeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed =
        dictionary_service::enabled_int_values(&state.db, "employmentStatus").await?;
    let education_allowed = dictionary_service::enabled_int_values(&state.db, "education").await?;
    employee_validate::validate_update_employee(&req, &status_allowed, &education_allowed)
        .map_err(AppError::Biz)?;

    let model = employee_service::update_employee(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], EmployeeResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 员工档案详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_employee(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<EmployeeResp> {
    let state = AppState::from_depot(depot)?;
    let model = employee_service::get_employee(&state.db, body.into_inner().id).await?;
    let resp = fill_user_names(&state.db, vec![model], EmployeeResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除员工档案（软删；关联登录账号保留，由用户管理单独处理）。
#[endpoint]
pub async fn delete_employee(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    employee_service::delete_employee(&state.db, auth.user_id, body.into_inner().id).await?;
    Ok(ApiResponse::ok(()))
}
