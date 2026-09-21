//! 刷新凭证 handler（Protected 三件套挂载：AuthRequired → OperationLog → ApiPermission）。

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::refresh_token::dto::{
    DeleteBatchReq, LogoutUserReq, RefreshTokenListReq, RefreshTokenResp,
};
use crate::modules::system::refresh_token::service as refresh_token_service;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 凭证列表（POST + JSON body）：在线会话/历史记录分页，`revoked_by` 一并带出操作人名称。
#[endpoint]
pub async fn list_refresh_tokens(
    depot: &mut Depot,
    body: JsonBody<RefreshTokenListReq>,
) -> ApiResult<PageResult<RefreshTokenResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = refresh_token_service::page_refresh_tokens(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, RefreshTokenResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 物理删除（POST + JSON body：`{ "id": ... }`）：仅限死记录（已吊销/已过期），
/// 活跃会话拒绝并提示先强制下线。
#[endpoint]
pub async fn delete_refresh_token(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    refresh_token_service::delete_refresh_token(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 批量物理删除（POST + JSON body：`{ "ids": [...] }`）：活跃记录静默跳过，
/// 返回受影响行数。
#[endpoint]
pub async fn delete_refresh_token_batch(
    depot: &mut Depot,
    body: JsonBody<DeleteBatchReq>,
) -> ApiResult<u64> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let affected = refresh_token_service::delete_refresh_token_batch(&state.db, &req.ids).await?;
    Ok(ApiResponse::ok(affected))
}

/// 强制下线（POST + JSON body：`{ "id": ... }`，吊销指定凭证并盖章操作人）。
#[endpoint]
pub async fn force_logout_refresh_token(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let actor = AuthUser::from_depot(depot)?;
    let req = body.into_inner();
    refresh_token_service::force_logout_refresh_token(
        &state.db,
        req.id,
        actor.user_id,
        "管理员强制下线",
    )
    .await?;
    Ok(ApiResponse::ok(()))
}

/// 按用户强制下线（POST + JSON body：`{ "userId": ... }`）：吊销该用户全部
/// 仍然有效的会话，返回受影响会话数（0 = 该用户没有在线会话）。
/// 允许管理员对自己操作——踢自己即立刻掉线，需重新登录。
#[endpoint]
pub async fn force_logout_user(depot: &mut Depot, body: JsonBody<LogoutUserReq>) -> ApiResult<u64> {
    let state = AppState::from_depot(depot)?;
    let actor = AuthUser::from_depot(depot)?;
    let req = body.into_inner();
    let affected = refresh_token_service::force_logout_user_sessions(
        &state.db,
        req.user_id,
        actor.user_id,
        "管理员强制下线",
    )
    .await?;
    Ok(ApiResponse::ok(affected))
}
