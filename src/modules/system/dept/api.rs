//! 部门 handler：树列表 + CRUD。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：`list → create → update → get → delete`。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::dept::dto::{CreateDeptReq, DeptResp, UpdateDeptReq};
use crate::modules::system::dept::service as dept_service;
use crate::modules::system::dept::validate as dept_validate;
use crate::modules::system::dictionary::service as dict_service;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq};

/// 部门树列表（POST，无分页）：children 递归，含停用节点；
/// 负责人（leaders）与审计人显示名由服务端批量拼装。
#[endpoint]
pub async fn list_depts(depot: &mut Depot) -> ApiResult<Vec<DeptResp>> {
    let state = AppState::from_depot(depot)?;
    let mut tree = dept_service::list_dept_tree(&state.db).await?;
    dept_service::fill_dept_audit_names(&state.db, &mut tree).await?;
    dept_service::fill_dept_leaders(&state.db, &mut tree).await?;
    Ok(ApiResponse::ok(tree))
}

/// 创建部门（POST + JSON body）：值域校验后交由 service 落库（含 path 回写）。
#[endpoint]
pub async fn create_dept(depot: &mut Depot, body: JsonBody<CreateDeptReq>) -> ApiResult<DeptResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    dept_validate::validate_create_dept(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = dept_service::create_dept(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(fill_single(&state.db, model).await?))
}

/// 更新部门（POST + JSON body）：`parent_id` 变更即移动子树并重算 path。
#[endpoint]
pub async fn update_dept(depot: &mut Depot, body: JsonBody<UpdateDeptReq>) -> ApiResult<DeptResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    dept_validate::validate_update_dept(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = dept_service::update_dept(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(fill_single(&state.db, model).await?))
}

/// 部门详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_dept(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<DeptResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = dept_service::get_dept(&state.db, req.id).await?;
    Ok(ApiResponse::ok(fill_single(&state.db, model).await?))
}

/// 删除部门（POST + JSON body：`{ "id": ... }`）：有子部门或用户挂载时拒绝，否则软删。
#[endpoint]
pub async fn delete_dept(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;
    dept_service::delete_dept(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 单节点响应拼装：审计人显示名 + 负责人列表（复用树的批量填充，节点无 children）。
async fn fill_single(
    db: &impl sea_orm::ConnectionTrait,
    model: crate::entity::sys_dept::Model,
) -> Result<DeptResp, AppError> {
    let mut nodes = vec![DeptResp::from(model)];
    dept_service::fill_dept_audit_names(db, &mut nodes).await?;
    dept_service::fill_dept_leaders(db, &mut nodes).await?;
    Ok(nodes.remove(0))
}
