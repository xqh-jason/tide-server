//! 菜单域 handler。

use salvo::oapi::endpoint;
use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::menu::dto::{
    CreateMenuReq, MenuIdReq, MenuListReq, MenuResp, UpdateMenuReq, VbenMenuItem,
};
use crate::modules::menu::service as menu_service;
use crate::utils::{ApiResponse, ApiResult, PageResult};

/// vben 菜单树（契约 §3.2），vben `fetchMenuListAsync` 消费后动态注册路由。
/// 挂载路径仍为 `POST /api/v1/user/menus`（router.rs 组装），
/// 按当前登录用户实时角色过滤（超管全量 / 普通用户按 `sys_role_menu` 绑定）。
/// 返回当前用户有权限访问的菜单树，排除软删除菜单。
#[endpoint]
pub async fn menus(depot: &mut Depot) -> ApiResult<Vec<VbenMenuItem>> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    let menu_tree = menu_service::get_menus(&state.db, auth.user_id).await?;
    Ok(ApiResponse::ok(menu_tree))
}

/// 菜单列表（POST + JSON body）：分页 + keyword / status / menu_type 过滤。
#[endpoint]
pub async fn list_menus(
    depot: &mut Depot,
    req: JsonBody<MenuListReq>,
) -> ApiResult<PageResult<MenuResp>> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let data = menu_service::page_menus(&state.db, &req).await?;
    Ok(ApiResponse::ok(data.into()))
}

/// 创建菜单（POST + JSON body）：name 全局唯一、component 格式校验，缺省字段用默认值。
#[endpoint]
pub async fn create_menu(depot: &mut Depot, req: JsonBody<CreateMenuReq>) -> ApiResult<MenuResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let menu = menu_service::create_menu(&state.db, &req).await?;
    Ok(ApiResponse::ok(MenuResp::from(menu)))
}

/// 更新菜单（POST + JSON body）：编辑表单全量提交，name 查重排除自身。
#[endpoint]
pub async fn update_menu(depot: &mut Depot, req: JsonBody<UpdateMenuReq>) -> ApiResult<MenuResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let menu = menu_service::update_menu(&state.db, &req).await?;
    Ok(ApiResponse::ok(MenuResp::from(menu)))
}

/// 菜单详情（POST + JSON body：`{ "id": ... }`）：按 id 查询单个菜单，不存在返回业务错误。
#[endpoint]
pub async fn get_menu(depot: &mut Depot, req: JsonBody<MenuIdReq>) -> ApiResult<MenuResp> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    let menu = menu_service::get_menu(&state.db, req.id).await?;
    Ok(ApiResponse::ok(MenuResp::from(menu)))
}

/// 删除菜单（POST + JSON body：`{ "id": ... }`）：级联软删全部子孙菜单并清空角色关联。
#[endpoint]
pub async fn delete_menu(depot: &mut Depot, req: JsonBody<MenuIdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = req.into_inner();
    menu_service::delete_menu(&state.db, req.id).await?;
    Ok(ApiResponse::ok(()))
}
