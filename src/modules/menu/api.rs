//! 菜单域 handler。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::menu::dto::VbenMenuItem;
use crate::modules::menu::service as menu_service;
use crate::utils::error::AppError;
use crate::utils::{ApiResponse, ApiResult};

/// vben 菜单树（契约 §3.2），vben `fetchMenuListAsync` 消费后动态注册路由。
/// 挂载路径仍为 `POST /api/v1/user/menus`（router.rs 组装）。
#[endpoint]
pub async fn menus(depot: &mut Depot) -> ApiResult<Vec<VbenMenuItem>> {
    let state = depot
        .get_typed::<AppState>()
        .map_err(|_| AppError::Biz("app state not found".into()))?;
    let auth = depot
        .get_typed::<AuthUser>()
        .map_err(|_| AppError::Biz("unauthorized".into()))?;
    let menu_tree = menu_service::get_menus(&state.db, &auth.roles).await?;
    Ok(ApiResponse::ok(menu_tree))
}
