//! 审批域 handler（端点函数）。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：模板 list → create → update → get → delete，
//! 节点 list → upsert → delete，实例 list → get → todo → approve → reject → cancel，记录 list。
//!
//! handler 只做四件事：取状态（`AppState`）/ 取操作人（`AuthUser`）→ 预取值域（字典）
//! → 调 validate + service → 拼显示名（`fill_user_names`）。业务规则不写在这里。
use salvo::oapi::endpoint;
use salvo::prelude::*;
use sea_orm::ConnectionTrait;

use crate::entity::{hr_approval_flow, hr_approval_instance};
use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::biz::hr::approval::dto::{
    ApproveReq, CreateFlowReq, FlowDetailResp, FlowListReq, FlowNodeListReq, FlowNodeResp,
    FlowResp, InstanceDetailResp, InstanceListReq, InstanceResp, RecordListReq, RecordResp,
    TodoListReq, UpdateFlowReq, UpsertFlowNodeReq,
};
use crate::modules::biz::hr::approval::service as approval_service;
use crate::modules::biz::hr::approval::{DICT_APPROVAL_BIZ_TYPE, validate};
use crate::modules::system::dictionary::service as dictionary_service;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 预取字典 `approvalBizType` 的启用项 `value`（业务类型允许值的唯一来源）。
async fn approval_biz_types(db: &impl ConnectionTrait) -> Result<Vec<String>, AppError> {
    let (_, details) =
        dictionary_service::get_dictionary_by_type(db, DICT_APPROVAL_BIZ_TYPE).await?;
    Ok(details.into_iter().map(|d| d.value).collect())
}

/// 单条模板响应：批量取名管道只吃 `Vec`，单条也走同一管道（保证与列表口径一致）。
async fn fill_one_flow(
    db: &impl ConnectionTrait,
    flow: hr_approval_flow::Model,
) -> Result<FlowResp, AppError> {
    let mut items = fill_user_names(db, vec![flow], FlowResp::from).await?;
    items
        .pop()
        .ok_or_else(|| AppError::Biz("审批流不存在".into()))
}

/// 单条实例响应（同上）。
async fn fill_one_instance(
    db: &impl ConnectionTrait,
    instance: hr_approval_instance::Model,
) -> Result<InstanceResp, AppError> {
    let mut items = fill_user_names(db, vec![instance], InstanceResp::from).await?;
    items
        .pop()
        .ok_or_else(|| AppError::Biz("审批实例不存在".into()))
}

/// 模板列表（POST + JSON body）。
#[endpoint]
pub async fn list_flows(
    depot: &mut Depot,
    body: JsonBody<FlowListReq>,
) -> ApiResult<PageResult<FlowResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = approval_service::page_flows(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, FlowResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建审批流模板。
#[endpoint]
pub async fn create_flow(depot: &mut Depot, body: JsonBody<CreateFlowReq>) -> ApiResult<FlowResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;
    let biz_type_allowed = approval_biz_types(&state.db).await?;
    validate::validate_create_flow(&req, &status_allowed, &biz_type_allowed)
        .map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let flow = approval_service::create_flow(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(fill_one_flow(&state.db, flow).await?))
}

/// 更新审批流模板。
#[endpoint]
pub async fn update_flow(depot: &mut Depot, body: JsonBody<UpdateFlowReq>) -> ApiResult<FlowResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;
    let biz_type_allowed = approval_biz_types(&state.db).await?;
    validate::validate_update_flow(&req, &status_allowed, &biz_type_allowed)
        .map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let flow = approval_service::update_flow(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(fill_one_flow(&state.db, flow).await?))
}

/// 审批流模板详情（含节点，按 `seq` 升序）。
#[endpoint]
pub async fn get_flow(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<FlowDetailResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let (flow, nodes) = approval_service::get_flow(&state.db, req.id).await?;
    Ok(ApiResponse::ok(FlowDetailResp {
        flow: fill_one_flow(&state.db, flow).await?,
        nodes: nodes.into_iter().map(FlowNodeResp::from).collect(),
    }))
}

/// 删除审批流模板（软删；有审批中的单据时拒绝）。
#[endpoint]
pub async fn delete_flow(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    approval_service::delete_flow(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 模板节点列表（按 `seq` 升序）。
#[endpoint]
pub async fn list_flow_nodes(
    depot: &mut Depot,
    body: JsonBody<FlowNodeListReq>,
) -> ApiResult<PageResult<FlowNodeResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = approval_service::page_flow_nodes(&state.db, &req).await?;
    let items = data.items.into_iter().map(FlowNodeResp::from).collect();
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 新增 / 修改模板节点（`id = 0` 新增）。
#[endpoint]
pub async fn upsert_flow_node(
    depot: &mut Depot,
    body: JsonBody<UpsertFlowNodeReq>,
) -> ApiResult<FlowNodeResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_upsert_flow_node(&req).map_err(AppError::Biz)?;
    let auth = AuthUser::from_depot(depot)?;
    let node = approval_service::upsert_flow_node(&state.db, auth.user_id, req).await?;
    Ok(ApiResponse::ok(FlowNodeResp::from(node)))
}

/// 删除模板节点（硬删；删除后最后一个节点不允许跳过）。
#[endpoint]
pub async fn delete_flow_node(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    approval_service::delete_flow_node(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 审批实例列表（管理视角：按业务类型 / 状态 / 申请人过滤）。
#[endpoint]
pub async fn list_instances(
    depot: &mut Depot,
    body: JsonBody<InstanceListReq>,
) -> ApiResult<PageResult<InstanceResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = approval_service::page_instances(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, InstanceResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 审批实例详情（含全部节点记录）。
#[endpoint]
pub async fn get_instance(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<InstanceDetailResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let (instance, records) = approval_service::get_instance(&state.db, req.id).await?;
    let records = fill_user_names(&state.db, records, RecordResp::from).await?;
    Ok(ApiResponse::ok(InstanceDetailResp {
        instance: fill_one_instance(&state.db, instance).await?,
        records,
    }))
}

/// 我的待办（当前节点轮到我 / 我持有该角色池角色的审批中实例）。
#[endpoint]
pub async fn list_todo_instances(
    depot: &mut Depot,
    body: JsonBody<TodoListReq>,
) -> ApiResult<PageResult<InstanceResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let data = approval_service::page_todo(&state.db, auth.user_id, &req).await?;
    let items = fill_user_names(&state.db, data.items, InstanceResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 审批通过（当前节点）。
#[endpoint]
pub async fn approve_instance(depot: &mut Depot, body: JsonBody<ApproveReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_approve(&req).map_err(AppError::Biz)?;
    let auth = AuthUser::from_depot(depot)?;
    approval_service::approve(&state.db, auth.user_id, req.id, &req.opinion).await?;
    Ok(ApiResponse::ok(()))
}

/// 审批驳回（当前节点；实例直接置已驳回）。
#[endpoint]
pub async fn reject_instance(depot: &mut Depot, body: JsonBody<ApproveReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_approve(&req).map_err(AppError::Biz)?;
    let auth = AuthUser::from_depot(depot)?;
    approval_service::reject(&state.db, auth.user_id, req.id, &req.opinion).await?;
    Ok(ApiResponse::ok(()))
}

/// 撤销实例（只有申请人本人可撤销审批中的单据）。
#[endpoint]
pub async fn cancel_instance(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    approval_service::cancel_instance(&state.db, auth.user_id, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 节点记录列表（按 `seq` 升序）。
#[endpoint]
pub async fn list_records(
    depot: &mut Depot,
    body: JsonBody<RecordListReq>,
) -> ApiResult<PageResult<RecordResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = approval_service::page_records(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, RecordResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}
