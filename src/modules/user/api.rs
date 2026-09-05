use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::user::dto::*;
use crate::modules::user::service as user_service;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::{UserRefNames, fill_user_names, find_user_name_map_by_ids};
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 用户列表（POST + JSON body）。分页字段（PageQuery）与过滤字段（keyword/status）
/// 通过 `#[serde(flatten)]` 合并为单个 `UserListReq`，一个 `JsonBody` 提取器取全部。
#[endpoint]
pub async fn list_users(
    depot: &mut Depot,
    body: JsonBody<UserListReq>,
) -> ApiResult<PageResult<UserResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = user_service::page_users(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, UserResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 按用户名查询用户（POST + JSON body：`{ "username": "..." }`）。
///
/// 说明：`ApiResult<T>` = `Result<ApiResponse<T>, AppError>`，Ok/Err 都实现 `Writer`，
/// 由 Salvo 自动渲染；`#[endpoint]` 据此生成 OpenAPI 文档。
#[endpoint]
pub async fn get_by_username(
    depot: &mut Depot,
    body: JsonBody<UsernameReq>,
) -> ApiResult<Option<UserResp>> {
    let state = AppState::from_depot(depot)?;
    let user = user_service::get_by_username(&state.db, &body.username).await?;
    let items = fill_user_names(&state.db, user.into_iter().collect(), UserResp::from).await?;
    Ok(ApiResponse::ok(items.into_iter().next()))
}

/// 当前登录用户信息（契约 §3.2）。认证中间件已注入 `AuthUser`。
#[endpoint]
pub async fn info(depot: &mut Depot) -> ApiResult<UserInfoResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let user = user_service::get_user_info(&state.db, auth.user_id).await?;
    let mut resp = UserInfoResp::from_model(user, &auth);
    // 填自身档案里的创建人/更新人显示名（能查到的只有操作自己的场景）
    let names = find_user_name_map_by_ids(&state.db, vec![resp.user_info.created_by]).await?;
    resp.user_info.set_user_ref_names(&names);
    Ok(ApiResponse::ok(resp))
}

/// 权限码数组（契约 §3.2），vben `getAccessCodes` 消费，`v-access` 判断按钮显隐。
#[endpoint]
pub async fn access_codes(depot: &mut Depot) -> ApiResult<Vec<String>> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let codes = user_service::get_access_codes(&state.db, auth.user_id).await?;
    Ok(ApiResponse::ok(codes))
}

/// 创建用户（POST + JSON body）。
#[endpoint]
pub async fn create_user(depot: &mut Depot, body: JsonBody<CreateUserReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let resp = user_service::create_user(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![resp], UserResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 获取用户详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_user(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let resp = user_service::get_user(&state.db, req.id).await?;
    let resp = fill_user_names(&state.db, vec![resp], UserResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新用户（POST + JSON body）。发起人需拥有 `system:user:update` 权限；
/// 内置超管 admin 不允许被编辑。
#[endpoint]
pub async fn update_user(depot: &mut Depot, body: JsonBody<UpdateUserReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;
    let resp = user_service::update_user_with_links(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![resp], UserResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新用户状态（POST + JSON body：`{ "id": ..., "status": ... }`）；
/// 内置超管 admin 不允许修改状态。
#[endpoint]
pub async fn update_user_status(
    depot: &mut Depot,
    body: JsonBody<UpdateUserStatusReq>,
) -> ApiResult<bool> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;
    let resp =
        user_service::update_user_status(&state.db, auth.user_id, req.id, req.status).await?;
    Ok(ApiResponse::ok(resp))
}
