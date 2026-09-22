//! 加班域 handler（端点函数）。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：list → create → update → get → delete → submit → cancel → mine。
//!
//! handler 只做四件事：取状态（`AppState`）/ 取操作人（`AuthUser`）→ 校验值域
//! → 调 validate + service → 拼显示名（`created_by_name` / `updated_by_name` 走平台管道
//! `fill_user_names`，`employee_name` 走本域 `fill_employee_names`）。业务规则不写在这里。
use salvo::oapi::endpoint;
use salvo::prelude::*;
use sea_orm::ConnectionTrait;

use crate::entity::hr_overtime_request;
use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::biz::hr::overtime::dto::{
    CreateOvertimeReq, MineReq, OvertimeListReq, OvertimeResp, UpdateOvertimeReq,
};
use crate::modules::biz::hr::overtime::service as overtime_service;
use crate::modules::biz::hr::overtime::validate;
use crate::utils::ApiResponse;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResult, IdReq, PageResult};

/// 给加班单响应批量回填 `employee_name`（一次批量查；禁止逐行查库）。
async fn fill_employee_names_of(
    db: &impl ConnectionTrait,
    items: &mut [OvertimeResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let names = overtime_service::fill_employee_names(db, &employee_ids).await?;
    for item in items.iter_mut() {
        item.employee_name = names.get(&item.employee_id).cloned().unwrap_or_default();
    }
    Ok(())
}

/// 单条加班单响应：批量取名管道只吃 `Vec`，单条也走同一管道（保证与列表口径一致）。
async fn fill_one_overtime(
    db: &impl ConnectionTrait,
    model: hr_overtime_request::Model,
) -> Result<OvertimeResp, AppError> {
    let mut items = fill_user_names(db, vec![model], OvertimeResp::from).await?;
    fill_employee_names_of(db, &mut items).await?;
    items
        .pop()
        .ok_or_else(|| AppError::Biz("加班单不存在".into()))
}

/// 加班单列表（管理视角：按员工 / 状态 / 类型 / 加班日期区间过滤）。
#[endpoint]
pub async fn list_overtimes(
    depot: &mut Depot,
    body: JsonBody<OvertimeListReq>,
) -> ApiResult<PageResult<OvertimeResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    // 列表过滤入参全仓一律透传（非法值自然查不到数据），不在这里校验
    let data = overtime_service::page_overtimes(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, OvertimeResp::from).await?;
    fill_employee_names_of(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建加班单（建单即提交审批；时长由后端按起止时间派生）。
#[endpoint]
pub async fn create_overtime(
    depot: &mut Depot,
    body: JsonBody<CreateOvertimeReq>,
) -> ApiResult<OvertimeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_create_overtime(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = overtime_service::create_overtime(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(fill_one_overtime(&state.db, model).await?))
}

/// 修改加班单（仅「已驳回 / 已撤销」可改，改完需重新提交）。
#[endpoint]
pub async fn update_overtime(
    depot: &mut Depot,
    body: JsonBody<UpdateOvertimeReq>,
) -> ApiResult<OvertimeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_update_overtime(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = overtime_service::update_overtime(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(fill_one_overtime(&state.db, model).await?))
}

/// 加班单详情。
#[endpoint]
pub async fn get_overtime(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<OvertimeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let model = overtime_service::get_overtime(&state.db, req.id).await?;
    Ok(ApiResponse::ok(fill_one_overtime(&state.db, model).await?))
}

/// 删除加班单（软删；审批中的单据先撤在途审批实例）。
#[endpoint]
pub async fn delete_overtime(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    overtime_service::delete_overtime(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 重新提交加班单（仅「已驳回 / 已撤销」）。
#[endpoint]
pub async fn submit_overtime(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<OvertimeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let model = overtime_service::submit_overtime(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(fill_one_overtime(&state.db, model).await?))
}

/// 撤销加班单（仅「审批中」；先置「已撤销」再撤在途审批实例）。
#[endpoint]
pub async fn cancel_overtime(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    overtime_service::cancel_overtime(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 我的加班单（申请人自助视角：只看当前用户员工档案下的单据）。
#[endpoint]
pub async fn list_my_overtimes(
    depot: &mut Depot,
    body: JsonBody<MineReq>,
) -> ApiResult<PageResult<OvertimeResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let data = overtime_service::page_my_overtimes(&state.db, auth.user_id, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, OvertimeResp::from).await?;
    fill_employee_names_of(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}
