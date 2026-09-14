//! 数据字典 handler。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：类型组 CRUD → `get-by-type`（特殊契约
//! 端点，排在 CRUD 之后）→ 字典项组 CRUD。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::dictionary::dto::{
    CreateDictionaryDetailReq, CreateDictionaryReq, DictionaryDetailListReq, DictionaryDetailResp,
    DictionaryListReq, DictionaryOptionResp, DictionaryResp, DictionaryTypeReq,
    UpdateDictionaryDetailReq, UpdateDictionaryReq,
};
use crate::modules::system::dictionary::service as dict_service;
use crate::modules::system::dictionary::validate as dict_validate;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

// —— 字典类型 ——

/// 字典类型列表（POST + JSON body）。
#[endpoint]
pub async fn list_dictionaries(
    depot: &mut Depot,
    body: JsonBody<DictionaryListReq>,
) -> ApiResult<PageResult<DictionaryResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = dict_service::page_dictionaries(&state.db, &req).await?;
    // 填充创建人和更新人显示名
    let items = fill_user_names(&state.db, data.items, DictionaryResp::from).await?;
    let page = crate::utils::PageResult::new(data.total, data.total_pages, items);
    Ok(ApiResponse::ok(page))
}

/// 创建字典类型（POST + JSON body）。
#[endpoint]
pub async fn create_dictionary(
    depot: &mut Depot,
    body: JsonBody<CreateDictionaryReq>,
) -> ApiResult<DictionaryResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    dict_validate::validate_create_dictionary(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = dict_service::create_dictionary(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], DictionaryResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新字典类型（POST + JSON body）。
#[endpoint]
pub async fn update_dictionary(
    depot: &mut Depot,
    body: JsonBody<UpdateDictionaryReq>,
) -> ApiResult<DictionaryResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    dict_validate::validate_update_dictionary(&req, &status_allowed).map_err(AppError::Biz)?;

    let model = dict_service::update_dictionary(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], DictionaryResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 字典类型详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_dictionary(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<DictionaryResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = dict_service::get_dictionary(&state.db, req.id).await?;
    let resp = fill_user_names(&state.db, vec![model], DictionaryResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除字典类型：级联软删其下字典项，返回删除数量。
#[endpoint]
pub async fn delete_dictionary(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<u64> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;
    let removed = dict_service::delete_dictionary(&state.db, req.id, auth.user_id).await?;
    Ok(ApiResponse::ok(removed))
}

/// 按类型编码取启用字典项（前端下拉契约，特殊端点）。
#[endpoint]
pub async fn get_dictionary_by_type(
    depot: &mut Depot,
    body: JsonBody<DictionaryTypeReq>,
) -> ApiResult<DictionaryOptionResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let (model, details) = dict_service::get_dictionary_by_type(&state.db, &req.r#type).await?;
    Ok(ApiResponse::ok(DictionaryOptionResp {
        id: model.id,
        name: model.name,
        r#type: model.r#type,
        details: details.into_iter().map(Into::into).collect(),
    }))
}

// —— 字典项 ——

/// 字典项列表（POST + JSON body）。
#[endpoint]
pub async fn list_dictionary_details(
    depot: &mut Depot,
    body: JsonBody<DictionaryDetailListReq>,
) -> ApiResult<PageResult<DictionaryDetailResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = dict_service::page_dictionary_details(&state.db, &req).await?;
    // 填充创建人和更新人显示名
    let items = fill_user_names(&state.db, data.items, DictionaryDetailResp::from).await?;
    let page = crate::utils::PageResult::new(data.total, data.total_pages, items);
    Ok(ApiResponse::ok(page))
}

/// 创建字典项（POST + JSON body）。
#[endpoint]
pub async fn create_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<CreateDictionaryDetailReq>,
) -> ApiResult<DictionaryDetailResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    dict_validate::validate_create_dictionary_detail(&req, &status_allowed)
        .map_err(AppError::Biz)?;

    let model = dict_service::create_dictionary_detail(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], DictionaryDetailResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新字典项（POST + JSON body）。
#[endpoint]
pub async fn update_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<UpdateDictionaryDetailReq>,
) -> ApiResult<DictionaryDetailResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let auth = AuthUser::from_depot(depot)?;

    let status_allowed = dict_service::enabled_int_values(&state.db, "status").await?;
    dict_validate::validate_update_dictionary_detail(&req, &status_allowed)
        .map_err(AppError::Biz)?;

    let model = dict_service::update_dictionary_detail(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], DictionaryDetailResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 字典项详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<DictionaryDetailResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let model = dict_service::get_dictionary_detail(&state.db, req.id).await?;
    let resp = fill_user_names(&state.db, vec![model], DictionaryDetailResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除字典项（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn delete_dictionary_detail(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    dict_service::delete_dictionary_detail(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
