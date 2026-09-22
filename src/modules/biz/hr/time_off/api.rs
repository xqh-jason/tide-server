//! 假期额度 handler（端点函数）。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：假期类型 list → create → update → get → delete，
//! 额度发放 list → batch-create → get → cancel，额度账户 list → get → logs。
//!
//! handler 只做三件事：取状态（`AppState`）/ 取操作人（`AuthUser`）→ 预取值域（字典）
//! → 调 validate + service → 拼显示名（`fill_user_names`；发放 / 账户行的
//! `employee_name` / `time_off_type_name` 另走本域 `fill_employee_names` /
//! `fill_time_off_type_names`）。业务规则不写在这里。
//!
//!
use salvo::oapi::endpoint;
use salvo::prelude::*;
use sea_orm::ConnectionTrait;

use crate::entity::hr_time_off_request;
use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::biz::hr::time_off::service as time_off_service;
use crate::modules::biz::hr::time_off::validate;
use crate::modules::system::dictionary::service as dictionary_service;
use crate::utils::ApiResponse;
use crate::utils::user_ref::fill_user_names;

use crate::modules::biz::hr::time_off::dto::{
    BatchCreateGrantReq, BatchCreateGrantResp, CreateTimeOffRequestReq, CreateTimeOffTypeReq,
    MineTimeOffRequestReq, TimeOffBalanceListReq, TimeOffBalanceLogListReq, TimeOffBalanceLogResp,
    TimeOffBalanceResp, TimeOffGrantListReq, TimeOffGrantResp, TimeOffRequestListReq,
    TimeOffRequestResp, TimeOffTypeListReq, TimeOffTypeResp, UpdateTimeOffRequestReq,
    UpdateTimeOffTypeReq,
};
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResult, IdReq, PageResult};

/// 给批次响应批量回填 `employee_name` / `time_off_type_name`（各一次批量查）。
async fn fill_grant_ref_names(
    db: &impl ConnectionTrait,
    items: &mut [TimeOffGrantResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let type_ids: Vec<u64> = items.iter().map(|item| item.time_off_type_id).collect();
    let employee_names = time_off_service::fill_employee_names(db, &employee_ids).await?;
    let type_names = time_off_service::fill_time_off_type_names(db, &type_ids).await?;

    for item in items.iter_mut() {
        item.employee_name = employee_names
            .get(&item.employee_id)
            .cloned()
            .unwrap_or_default();
        item.time_off_type_name = type_names
            .get(&item.time_off_type_id)
            .cloned()
            .unwrap_or_default();
    }
    Ok(())
}

/// 给账户响应批量回填 `employee_name` / `time_off_type_name`（各一次批量查）。
async fn fill_balance_ref_names(
    db: &impl ConnectionTrait,
    items: &mut [TimeOffBalanceResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let type_ids: Vec<u64> = items.iter().map(|item| item.time_off_type_id).collect();
    let employee_names = time_off_service::fill_employee_names(db, &employee_ids).await?;
    let type_names = time_off_service::fill_time_off_type_names(db, &type_ids).await?;

    for item in items.iter_mut() {
        item.employee_name = employee_names
            .get(&item.employee_id)
            .cloned()
            .unwrap_or_default();
        item.time_off_type_name = type_names
            .get(&item.time_off_type_id)
            .cloned()
            .unwrap_or_default();
    }
    Ok(())
}

/// 给流水响应批量回填 `employee_name` / `time_off_type_name`（`operator_name` 由
/// `fill_user_names` 走 `UserRefNames` 的 `operator_id` 口径填）。
async fn fill_log_ref_names(
    db: &impl ConnectionTrait,
    items: &mut [TimeOffBalanceLogResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let type_ids: Vec<u64> = items.iter().map(|item| item.time_off_type_id).collect();
    let employee_names = time_off_service::fill_employee_names(db, &employee_ids).await?;
    let type_names = time_off_service::fill_time_off_type_names(db, &type_ids).await?;

    for item in items.iter_mut() {
        item.employee_name = employee_names
            .get(&item.employee_id)
            .cloned()
            .unwrap_or_default();
        item.time_off_type_name = type_names
            .get(&item.time_off_type_id)
            .cloned()
            .unwrap_or_default();
    }
    Ok(())
}

/// 假期类型列表（POST + JSON body）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let req = body.into_inner();`；③
// `time_off_service::page_time_off_types(&state.db, &req).await?`；④ 人名：
// `fill_user_names(&state.db, data.items, TimeOffTypeResp::from).await?`；
// ⑤ `Ok(ApiResponse::ok(PageResult::new(data.total, data.total_pages, items)))`。
#[endpoint]
pub async fn list_time_off_types(
    depot: &mut Depot,
    body: JsonBody<TimeOffTypeListReq>,
) -> ApiResult<PageResult<TimeOffTypeResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = time_off_service::page_time_off_types(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, TimeOffTypeResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建假期类型。
//
// 实现提示：① `AppState::from_depot(depot)?` 取 db；② `let req = body.into_inner();`；③
// 值域预取（status 允许值的唯一来源是字典，不硬编码）：
// `let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;`；
// ④ `validate::validate_create_time_off_type(&req, &status_allowed).map_err(AppError::Biz)?;`；
// ⑤ `let auth = AuthUser::from_depot(depot)?;`；⑥
// `time_off_service::create_time_off_type(&state.db, auth.user_id, req).await?`；⑦ 人名：
// `fill_user_names(&state.db, vec![model], TimeOffTypeResp::from).await?.remove(0)`；⑧ `Ok(ApiResponse::ok(resp))`。
#[endpoint]
pub async fn create_time_off_type(
    depot: &mut Depot,
    body: JsonBody<CreateTimeOffTypeReq>,
) -> ApiResult<TimeOffTypeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    // status 允许值的唯一来源是平台字典，不硬编码
    let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;
    validate::validate_create_time_off_type(&req, &status_allowed).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = time_off_service::create_time_off_type(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![model], TimeOffTypeResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新假期类型。
//
// 实现提示：同 [`create_time_off_type`]，仅三处不同——④ 调
// `validate::validate_update_time_off_type(&req, &status_allowed)`（多 id 检查）；
// ⑥ 调 `time_off_service::update_time_off_type(&state.db, auth.user_id, &req)`（借用 req，不 move）；
// ⑦ 人名拼装同款。
#[endpoint]
pub async fn update_time_off_type(
    depot: &mut Depot,
    body: JsonBody<UpdateTimeOffTypeReq>,
) -> ApiResult<TimeOffTypeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;
    validate::validate_update_time_off_type(&req, &status_allowed).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = time_off_service::update_time_off_type(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], TimeOffTypeResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 假期类型详情（POST + JSON body：`{ "id": ... }`）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `time_off_service::get_time_off_type(&state.db, body.id).await?`；
// ③ 人名 `fill_user_names(&state.db, vec![model], TimeOffTypeResp::from).await?.remove(0)`；
// ④ `Ok(ApiResponse::ok(resp))`。
#[endpoint]
pub async fn get_time_off_type(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<TimeOffTypeResp> {
    let state = AppState::from_depot(depot)?;
    let model = time_off_service::get_time_off_type(&state.db, body.into_inner().id).await?;
    let resp = fill_user_names(&state.db, vec![model], TimeOffTypeResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除假期类型（软删）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let auth = AuthUser::from_depot(depot)?;`；
// ③ `time_off_service::delete_time_off_type(&state.db, auth.user_id, body.id).await?`；
// ④ `Ok(ApiResponse::ok(()))`。
#[endpoint]
pub async fn delete_time_off_type(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    time_off_service::delete_time_off_type(&state.db, auth.user_id, body.into_inner().id).await?;
    Ok(ApiResponse::ok(()))
}

/// 额度批次列表（POST + JSON body）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let req = body.into_inner();`；③
// `time_off_service::page_time_off_grants(&state.db, &req).await?`；④ 人名三路回填：
// `fill_employee_names` / `fill_time_off_type_names` 给 `employee_name` / `time_off_type_name`
// （返回 `HashMap<u64, String>`，遍历 items 写回），`created_by_name` / `updated_by_name` 走
// `fill_user_names(&state.db, items, TimeOffGrantResp::from).await?`；⑤ `Ok(ApiResponse::ok(PageResult::new(...)))`。
#[endpoint]
pub async fn list_time_off_grants(
    depot: &mut Depot,
    body: JsonBody<TimeOffGrantListReq>,
) -> ApiResult<PageResult<TimeOffGrantResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = time_off_service::page_time_off_grants(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, TimeOffGrantResp::from).await?;
    fill_grant_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 批量发放额度（部分员工命中幂等键 → 走 `skipped` 回执，不算失败）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let req = body.into_inner();`；③
// `validate::validate_batch_create_grant(&req).map_err(AppError::Biz)?;`（发放范围 / 时长 /
// 依据 / 周期 / 日期格式全在 validate，无需预取字典）；④ `let auth = AuthUser::from_depot(depot)?;`；
// ⑤ `time_off_service::batch_create_grants(&state.db, auth.user_id, &req).await?`；⑥ `Ok(ApiResponse::ok(resp))`。
#[endpoint]
pub async fn batch_create_time_off_grants(
    depot: &mut Depot,
    body: JsonBody<BatchCreateGrantReq>,
) -> ApiResult<BatchCreateGrantResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_batch_create_grant(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let resp = time_off_service::batch_create_grants(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(resp))
}

/// 额度批次详情（POST + JSON body：`{ "id": ... }`）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `time_off_service::get_time_off_grant(&state.db, body.id).await?`；
// ③ 人名回填同 [`list_time_off_grants`]（单条：`fill_employee_names` / `fill_time_off_type_names` 各取
// 1 个 id + `fill_user_names`）；④ `Ok(ApiResponse::ok(resp))`。
#[endpoint]
pub async fn get_time_off_grant(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<TimeOffGrantResp> {
    let state = AppState::from_depot(depot)?;
    let model = time_off_service::get_time_off_grant(&state.db, body.into_inner().id).await?;
    let mut items = fill_user_names(&state.db, vec![model], TimeOffGrantResp::from).await?;
    fill_grant_ref_names(&state.db, &mut items).await?;
    let resp = items.remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 撤销额度批次（账户回冲 + 反向流水，均在 service 事务内）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let auth = AuthUser::from_depot(depot)?;`；
// ③ `time_off_service::cancel_grant(&state.db, auth.user_id, body.id).await?`；④ `Ok(ApiResponse::ok(()))`。
#[endpoint]
pub async fn cancel_time_off_grant(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    time_off_service::cancel_grant(&state.db, auth.user_id, body.into_inner().id).await?;
    Ok(ApiResponse::ok(()))
}

/// 额度账户列表（POST + JSON body）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let req = body.into_inner();`；③
// `time_off_service::page_time_off_balances(&state.db, &req).await?`；④ 人名三路回填同
// [`list_time_off_grants`]（`TimeOffBalanceResp`）；⑤ `Ok(ApiResponse::ok(PageResult::new(...)))`。
#[endpoint]
pub async fn list_time_off_balances(
    depot: &mut Depot,
    body: JsonBody<TimeOffBalanceListReq>,
) -> ApiResult<PageResult<TimeOffBalanceResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = time_off_service::page_time_off_balances(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, TimeOffBalanceResp::from).await?;
    fill_balance_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 额度账户详情（POST + JSON body：`{ "id": ... }`）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `time_off_service::get_time_off_balance(&state.db, body.id).await?`；
// ③ 人名回填同 [`get_time_off_grant`]；④ `Ok(ApiResponse::ok(resp))`。
#[endpoint]
pub async fn get_time_off_balance(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<TimeOffBalanceResp> {
    let state = AppState::from_depot(depot)?;
    let model = time_off_service::get_time_off_balance(&state.db, body.into_inner().id).await?;
    let mut items = fill_user_names(&state.db, vec![model], TimeOffBalanceResp::from).await?;
    fill_balance_ref_names(&state.db, &mut items).await?;
    let resp = items.remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 额度流水查询（POST + JSON body；append-only 对账凭据）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let req = body.into_inner();`；③
// `time_off_service::page_time_off_balance_logs(&state.db, &req).await?`；④ 人名：
// `employee_name` / `time_off_type_name` 走 `fill_employee_names` / `fill_time_off_type_names`，
// `operator_name` 走 `fill_user_names(&state.db, items, TimeOffBalanceLogResp::from).await?`
// （流水的唯一人字段是 `operator_id`，0 = 系统，查不到给空串）；⑤ `Ok(ApiResponse::ok(PageResult::new(...)))`。
#[endpoint]
pub async fn list_time_off_balance_logs(
    depot: &mut Depot,
    body: JsonBody<TimeOffBalanceLogListReq>,
) -> ApiResult<PageResult<TimeOffBalanceLogResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = time_off_service::page_time_off_balance_logs(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, TimeOffBalanceLogResp::from).await?;
    fill_log_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

// —— 请假单（P2）——

/// 给请假单响应批量回填 `employee_name` / `time_off_type_name`（各一次批量查）。
async fn fill_request_ref_names(
    db: &impl ConnectionTrait,
    items: &mut [TimeOffRequestResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let type_ids: Vec<u64> = items.iter().map(|item| item.time_off_type_id).collect();
    let employee_names = time_off_service::fill_employee_names(db, &employee_ids).await?;
    let type_names = time_off_service::fill_time_off_type_names(db, &type_ids).await?;

    for item in items.iter_mut() {
        item.employee_name = employee_names
            .get(&item.employee_id)
            .cloned()
            .unwrap_or_default();
        item.time_off_type_name = type_names
            .get(&item.time_off_type_id)
            .cloned()
            .unwrap_or_default();
    }
    Ok(())
}

/// 单条请假单响应：批量取名管道只吃 `Vec`，单条也走同一管道（口径与列表一致）。
async fn build_request_resp(
    db: &impl ConnectionTrait,
    request: hr_time_off_request::Model,
) -> Result<TimeOffRequestResp, AppError> {
    let mut items = fill_user_names(db, vec![request], TimeOffRequestResp::from).await?;
    fill_request_ref_names(db, &mut items).await?;
    items
        .pop()
        .ok_or_else(|| AppError::Biz("请假单不存在".into()))
}

/// 请假单列表（管理视角：按员工 / 假别 / 状态 / 起止时间过滤）。
#[endpoint]
pub async fn list_time_off_requests(
    depot: &mut Depot,
    body: JsonBody<TimeOffRequestListReq>,
) -> ApiResult<PageResult<TimeOffRequestResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = time_off_service::page_time_off_requests(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, TimeOffRequestResp::from).await?;
    fill_request_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建请假单（建单即提交：预占额度 + 起审批实例，同一事务）。
#[endpoint]
pub async fn create_time_off_request(
    depot: &mut Depot,
    body: JsonBody<CreateTimeOffRequestReq>,
) -> ApiResult<TimeOffRequestResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_create_time_off_request(&req).map_err(AppError::Biz)?;
    let auth = AuthUser::from_depot(depot)?;
    let request = time_off_service::create_time_off_request(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(
        build_request_resp(&state.db, request).await?,
    ))
}

/// 修改请假单（仅「已驳回 / 已撤销」可改）。
#[endpoint]
pub async fn update_time_off_request(
    depot: &mut Depot,
    body: JsonBody<UpdateTimeOffRequestReq>,
) -> ApiResult<TimeOffRequestResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_update_time_off_request(&req).map_err(AppError::Biz)?;
    let auth = AuthUser::from_depot(depot)?;
    let request = time_off_service::update_time_off_request(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(
        build_request_resp(&state.db, request).await?,
    ))
}

/// 请假单详情。
#[endpoint]
pub async fn get_time_off_request(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<TimeOffRequestResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let request = time_off_service::get_time_off_request(&state.db, req.id).await?;
    Ok(ApiResponse::ok(
        build_request_resp(&state.db, request).await?,
    ))
}

/// 删除请假单（软删；审批中需先撤销）。
#[endpoint]
pub async fn delete_time_off_request(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    time_off_service::delete_time_off_request(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 重新提交请假单（仅「已驳回 / 已撤销」可提交）。
#[endpoint]
pub async fn submit_time_off_request(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<TimeOffRequestResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let request =
        time_off_service::submit_time_off_request(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(
        build_request_resp(&state.db, request).await?,
    ))
}

/// 撤销请假单（只有申请人本人、且审批中）。
#[endpoint]
pub async fn cancel_time_off_request(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<TimeOffRequestResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let request =
        time_off_service::cancel_time_off_request(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(
        build_request_resp(&state.db, request).await?,
    ))
}

/// 我的请假单（按登录用户对应的员工档案过滤）。
#[endpoint]
pub async fn list_my_time_off_requests(
    depot: &mut Depot,
    body: JsonBody<MineTimeOffRequestReq>,
) -> ApiResult<PageResult<TimeOffRequestResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let data = time_off_service::page_my_time_off_requests(&state.db, auth.user_id, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, TimeOffRequestResp::from).await?;
    fill_request_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}
