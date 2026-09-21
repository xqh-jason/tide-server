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
//! **本文件当前是桩**：函数体一律返回 `Err(AppError::Biz("未实现：<函数名>"))`，
//! 由作者按每个函数的 `// 实现提示` 补齐；端点行为随任务 5 端到端验收。
//!
//! 骨架期口径：`use` 只写**签名本身需要**的项（`AppError` 例外——桩函数体就在用它）；
//! 实现才需要的 import 刻意不写，以免触发 `-D warnings` 的未使用 import，清单见下：
//!
//! ```ignore
//! use crate::infra::state::AppState;
//! use crate::middleware::auth::AuthUser;
//! use crate::modules::biz::hr::time_off::service as time_off_service;
//! use crate::modules::biz::hr::time_off::validate;
//! use crate::modules::system::dictionary::service as dictionary_service;
//! use crate::utils::ApiResponse;
//! use crate::utils::user_ref::fill_user_names;
//! ```

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::modules::biz::hr::time_off::dto::{
    BatchCreateGrantReq, BatchCreateGrantResp, CreateTimeOffTypeReq, TimeOffBalanceListReq,
    TimeOffBalanceLogListReq, TimeOffBalanceLogResp, TimeOffBalanceResp, TimeOffGrantListReq,
    TimeOffGrantResp, TimeOffTypeListReq, TimeOffTypeResp, UpdateTimeOffTypeReq,
};
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResult, IdReq, PageResult};

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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：list_time_off_types".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：create_time_off_type".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：update_time_off_type".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：get_time_off_type".into()))
}

/// 删除假期类型（软删）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let auth = AuthUser::from_depot(depot)?;`；
// ③ `time_off_service::delete_time_off_type(&state.db, auth.user_id, body.id).await?`；
// ④ `Ok(ApiResponse::ok(()))`。
#[endpoint]
pub async fn delete_time_off_type(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let _ = (depot, body);
    Err(AppError::Biz("未实现：delete_time_off_type".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：list_time_off_grants".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：batch_create_time_off_grants".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：get_time_off_grant".into()))
}

/// 撤销额度批次（账户回冲 + 反向流水，均在 service 事务内）。
//
// 实现提示：① `AppState::from_depot(depot)?`；② `let auth = AuthUser::from_depot(depot)?;`；
// ③ `time_off_service::cancel_grant(&state.db, auth.user_id, body.id).await?`；④ `Ok(ApiResponse::ok(()))`。
#[endpoint]
pub async fn cancel_time_off_grant(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let _ = (depot, body);
    Err(AppError::Biz("未实现：cancel_time_off_grant".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：list_time_off_balances".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：get_time_off_balance".into()))
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
    let _ = (depot, body);
    Err(AppError::Biz("未实现：list_time_off_balance_logs".into()))
}
