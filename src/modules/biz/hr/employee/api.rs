//! 员工档案 handler（端点函数）。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：list → create → update → get → delete。
//!
//! handler 只做三件事：取状态（`AppState`）/ 取操作人（`AuthUser`）→ 预取值域（字典）
//! → 调 validate + service → 拼显示名（`fill_user_names`）。业务规则不写在这里。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::modules::biz::hr::employee::dto::{
    CreateEmployeeReq, EmployeeListReq, EmployeeResp, UpdateEmployeeReq,
};
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResult, IdReq, PageResult};

/// 员工档案列表（POST + JSON body）。
#[endpoint]
pub async fn list_employees(
    depot: &mut Depot,
    body: JsonBody<EmployeeListReq>,
) -> ApiResult<PageResult<EmployeeResp>> {
    // 实现提示：
    // 1) `let state = AppState::from_depot(depot)?;`
    //    （`use crate::infra::state::AppState;`）
    // 2) `let data = employee_service::page_employees(&state.db, &body.into_inner()).await?;`
    //    （`use crate::modules::biz::hr::employee::service as employee_service;`）
    // 3) `let items = fill_user_names(&state.db, data.items, EmployeeResp::from).await?;`
    //    （`use crate::utils::user_ref::fill_user_names;`）
    // 4) `Ok(ApiResponse::ok(PageResult::new(data.total, data.total_pages, items)))`
    //    （`use crate::utils::ApiResponse;`）
    let _ = (depot, body);
    Err(AppError::Biz("未实现：list_employees".into()))
}

/// 创建员工档案（可选同事务创建登录账号）。
#[endpoint]
pub async fn create_employee(
    depot: &mut Depot,
    body: JsonBody<CreateEmployeeReq>,
) -> ApiResult<EmployeeResp> {
    // 实现提示：
    // 1) `let state = AppState::from_depot(depot)?;`
    //    `let auth = AuthUser::from_depot(depot)?;`（`use crate::middleware::auth::AuthUser;`）
    // 2) 值域预取：
    //    `let status_allowed = dictionary_service::enabled_int_values(&state.db, "employmentStatus").await?;`
    //    `let education_allowed = dictionary_service::enabled_int_values(&state.db, "education").await?;`
    //    （`use crate::modules::system::dictionary::service as dictionary_service;`）
    // 3) `let req = body.into_inner();`
    //    `employee_validate::validate_create_employee(&req, &status_allowed, &education_allowed)`
    //      `.map_err(AppError::Biz)?;`（`use …::validate as employee_validate;`）
    // 4) `let model = employee_service::create_employee(&state.db, auth.user_id, req).await?;`
    // 5) `let items = fill_user_names(&state.db, vec![model], EmployeeResp::from).await?;`
    //    `Ok(ApiResponse::ok(items.into_iter().next().unwrap_or_else(|| unreachable!())))`
    //    —— 或先 `let resp = EmployeeResp::from(model);` 再单条拼装
    let _ = (depot, body);
    Err(AppError::Biz("未实现：create_employee".into()))
}

/// 更新员工档案（不修改关联账号；敏感字段空串 = 不修改）。
#[endpoint]
pub async fn update_employee(
    depot: &mut Depot,
    body: JsonBody<UpdateEmployeeReq>,
) -> ApiResult<EmployeeResp> {
    // 实现提示：同 create_employee，改 validate_update_employee
    //   与 `employee_service::update_employee(&state.db, auth.user_id, &req)`（注意借引用）
    let _ = (depot, body);
    Err(AppError::Biz("未实现：update_employee".into()))
}

/// 员工档案详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_employee(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<EmployeeResp> {
    // 实现提示：`employee_service::get_employee(&state.db, body.into_inner().id).await?`
    //   → `fill_user_names` 单条 → `ApiResponse::ok`
    let _ = (depot, body);
    Err(AppError::Biz("未实现：get_employee".into()))
}

/// 删除员工档案（软删；关联登录账号保留，由用户管理单独处理）。
#[endpoint]
pub async fn delete_employee(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    // 实现提示：`let auth = AuthUser::from_depot(depot)?;`
    //   `employee_service::delete_employee(&state.db, auth.user_id, body.into_inner().id).await?`
    //   `Ok(ApiResponse::ok(()))`
    let _ = (depot, body);
    Err(AppError::Biz("未实现：delete_employee".into()))
}
