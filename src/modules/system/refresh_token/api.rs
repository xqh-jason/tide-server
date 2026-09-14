//! 刷新凭证 handler（Protected 三件套挂载：AuthRequired → OperationLog → ApiPermission）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::refresh_token::dto::{
    DeleteBatchReq, RefreshTokenListReq, RefreshTokenResp,
};
use crate::modules::system::refresh_token::service as refresh_token_service;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 凭证列表（POST + JSON body）：在线会话/历史记录分页。
#[endpoint]
pub async fn list_refresh_tokens(
    depot: &mut Depot,
    body: JsonBody<RefreshTokenListReq>,
) -> ApiResult<PageResult<RefreshTokenResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = refresh_token_service::page_refresh_tokens(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
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
