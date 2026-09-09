//! 系统配置 handler。
//!
//! 端点顺序：参数 `list → create → update → get → delete`（POST + JSON body）；
//! 网站设置 `get`（GET，公开）、`update`（POST，登录态，中间件在 site_routes 子路由）。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::config::dto::{
    ConfigListReq, ConfigResp, CreateConfigReq, SiteConfigResp, UpdateConfigReq,
    UpdateSiteConfigReq,
};
use crate::modules::config::service as config_service;
use crate::modules::config::validate as config_validate;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 参数列表（POST + JSON body）。
#[endpoint]
pub async fn list_configs(
    depot: &mut Depot,
    body: JsonBody<ConfigListReq>,
) -> ApiResult<PageResult<ConfigResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = config_service::page_configs(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, ConfigResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建参数（POST + JSON body，config_key 全局唯一）。
#[endpoint]
pub async fn create_config(
    depot: &mut Depot,
    body: JsonBody<CreateConfigReq>,
) -> ApiResult<ConfigResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();
    config_validate::validate_create_config(&req).map_err(AppError::Biz)?;
    let model = config_service::create_config(&state.db, auth.user_id, &req).await?;
    let mut resp = fill_user_names(&state.db, vec![model], ConfigResp::from).await?;
    Ok(ApiResponse::ok(resp.remove(0)))
}

/// 更新参数（POST + JSON body，key 唯一排除自身）。
#[endpoint]
pub async fn update_config(
    depot: &mut Depot,
    body: JsonBody<UpdateConfigReq>,
) -> ApiResult<ConfigResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();
    config_validate::validate_update_config(&req).map_err(AppError::Biz)?;
    let model = config_service::update_config(&state.db, auth.user_id, &req).await?;
    let mut resp = fill_user_names(&state.db, vec![model], ConfigResp::from).await?;
    Ok(ApiResponse::ok(resp.remove(0)))
}

/// 参数详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_config(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<ConfigResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = config_service::get_config(&state.db, req.id).await?;
    let mut resp = fill_user_names(&state.db, vec![model], ConfigResp::from).await?;
    Ok(ApiResponse::ok(resp.remove(0)))
}

/// 删除参数（POST + JSON body：`{ "id": ... }`，软删）。
#[endpoint]
pub async fn delete_config(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    config_service::delete_config(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}

/// 网站设置读取（GET，公开端点：登录页展示站点名 / logo）。
///
/// 「POST + JSON body」契约的例外：未登录页面（如登录页）无法携带 token，
/// 用 GET 最直白；响应不含审计人字段（SiteConfigResp 本就不带）。
#[endpoint]
pub async fn get_site_config(depot: &mut Depot) -> ApiResult<SiteConfigResp> {
    let state = AppState::from_depot(depot)?;
    let model = config_service::get_site_config(&state.db).await?;
    Ok(ApiResponse::ok(model.into()))
}

/// 网站设置更新（POST + JSON body，全量提交，恒更新 id=1；中间件已在 site_routes 挂）。
#[endpoint]
pub async fn update_site_config(
    depot: &mut Depot,
    body: JsonBody<UpdateSiteConfigReq>,
) -> ApiResult<SiteConfigResp> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let req = body.into_inner();
    config_validate::validate_update_site_config(&req).map_err(AppError::Biz)?;
    let model = config_service::update_site_config(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(model.into()))
}
