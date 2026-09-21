use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::dictionary::service as dict_service;
use crate::modules::system::user::dto::*;
use crate::modules::system::user::service as user_service;
use crate::modules::system::user::validate as user_validate;
use crate::utils::error::AppError;
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
    let mut items = fill_user_names(&state.db, data.items, UserResp::from).await?;
    user_service::fill_user_dept_names(&state.db, &mut items).await?;
    user_service::fill_user_position_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 按用户名查询用户（POST + JSON body：`{ "username": "..." }`）。
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

/// 当前登录用户信息。认证中间件已注入 `AuthUser`。
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

/// 权限码数组，vben `getAccessCodes` 消费，`v-access` 判断按钮显隐。
#[endpoint]
pub async fn access_codes(depot: &mut Depot) -> ApiResult<Vec<String>> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let codes = user_service::get_access_codes(&state.db, auth.user_id).await?;
    Ok(ApiResponse::ok(codes))
}

/// 创建用户（POST + JSON body）。值域校验在拿到 `req` 后显式调用
/// `validate_create_user`（见同模块 validate.rs）。
#[endpoint]
pub async fn create_user(depot: &mut Depot, body: JsonBody<CreateUserReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    user_validate::validate_create_user(&req, &status_allowed).map_err(AppError::Biz)?;

    let resp = user_service::create_user(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![resp], UserResp::from)
        .await?
        .remove(0);
    let mut vec_resp = vec![resp];
    user_service::fill_user_dept_names(&state.db, &mut vec_resp).await?;
    user_service::fill_user_position_names(&state.db, &mut vec_resp).await?;
    Ok(ApiResponse::ok(vec_resp.remove(0)))
}

/// 获取用户详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_user(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let resp = user_service::get_user(&state.db, req.id).await?;
    let mut resp = fill_user_names(&state.db, vec![resp], UserResp::from)
        .await?
        .remove(0);
    resp.role_ids = user_service::get_role_ids_by_user_id(&state.db, resp.id).await?;
    let mut vec_resp = vec![resp];
    user_service::fill_user_dept_names(&state.db, &mut vec_resp).await?;
    user_service::fill_user_position_names(&state.db, &mut vec_resp).await?;
    Ok(ApiResponse::ok(vec_resp.remove(0)))
}

/// 更新用户（POST + JSON body）。发起人需拥有 `system:user:update` 权限；
/// 内置超管 admin 不允许被编辑。
#[endpoint]
pub async fn update_user(depot: &mut Depot, body: JsonBody<UpdateUserReq>) -> ApiResult<UserResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    user_validate::validate_update_user(&req, &status_allowed).map_err(AppError::Biz)?;

    let resp = user_service::update_user_with_links(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![resp], UserResp::from)
        .await?
        .remove(0);
    let mut vec_resp = vec![resp];
    user_service::fill_user_dept_names(&state.db, &mut vec_resp).await?;
    user_service::fill_user_position_names(&state.db, &mut vec_resp).await?;
    Ok(ApiResponse::ok(vec_resp.remove(0)))
}

/// 更新用户状态（POST + JSON body：`{ "id": ..., "status": ... }`）；
/// 内置超管 admin 不允许修改状态。
#[endpoint]
pub async fn update_user_status(
    depot: &mut Depot,
    body: JsonBody<UpdateUserStatusReq>,
) -> ApiResult<bool> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    user_validate::validate_update_user_status(&req, &status_allowed).map_err(AppError::Biz)?;

    let resp =
        user_service::update_user_status(&state.db, auth.user_id, req.id, req.status).await?;
    Ok(ApiResponse::ok(resp))
}

/// 删除用户（POST + JSON body：`{ "id": ... }`）：内置超管 admin 不允许删除；
/// 软删并清空角色关联。接口级权限码由后续 API 授权层统一施加。
#[endpoint]
pub async fn delete_user(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    user_service::delete_user(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 全量用户（排除软删）：用户管理场景的下拉数据源。
#[endpoint]
pub async fn list_all_users(depot: &mut Depot) -> ApiResult<Vec<UserResp>> {
    let state = AppState::from_depot(depot)?;
    let users = user_service::list_all_users(&state.db).await?;
    let mut vec_resp = users.into_iter().map(UserResp::from).collect::<Vec<_>>();
    user_service::fill_user_dept_names(&state.db, &mut vec_resp).await?;
    user_service::fill_user_position_names(&state.db, &mut vec_resp).await?;
    Ok(ApiResponse::ok(vec_resp))
}

/// 全量用户（含软删）：审计过滤的用户选择器数据源，仅暴露 id/username/deleted 简要字段。
#[endpoint]
pub async fn list_all_users_includes_soft_deleted(
    depot: &mut Depot,
) -> ApiResult<Vec<UserBriefResp>> {
    let state = AppState::from_depot(depot)?;
    let users = user_service::list_all_users_includes_soft_deleted(&state.db).await?;
    Ok(ApiResponse::ok(
        users.into_iter().map(UserBriefResp::from).collect(),
    ))
}

#[endpoint]
pub async fn get_depts_by_user_id(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<Vec<UserDeptResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let depts = user_service::get_depts_by_user_id(&state.db, req.id).await?;
    Ok(ApiResponse::ok(depts))
}

/// 某用户挂载的职位列表（含职位名）：用户详情 / 表单回显用（与 `get-depts` 对称）。
#[endpoint]
pub async fn get_positions_by_user_id(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<Vec<UserPositionResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let positions = user_service::get_positions_by_user_id(&state.db, req.id).await?;
    Ok(ApiResponse::ok(positions))
}
