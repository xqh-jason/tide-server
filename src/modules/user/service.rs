use std::collections::HashMap;

use crate::entity::{sys_menu, sys_user};
use crate::middleware::auth::AuthUser;
use crate::modules::user::repo as user_repo;
use crate::modules::user::dto::{UserInfoResp, VbenMenuItem, VbenMenuMeta};
use crate::utils::error::AppError;

pub async fn get_by_username(
    db: &sea_orm::DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<sys_user::Model>> {
    let user = user_repo::find_by_username(db, username).await?;

    Ok(user)
}

/// 分页查询用户。`page_index` 为 0-based（由 handler 层从 PageQuery 转换）。
pub async fn page_users(
    db: &sea_orm::DatabaseConnection,
    keyword: Option<String>,
    status: Option<i8>,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<(u64, Vec<sys_user::Model>)> {
    user_repo::find_page(db, keyword, status, page_index, page_size).await
}

/// 当前登录用户完整信息（契约 §3.2 的 `/user/info`）。
pub async fn get_user_info(
    db: &sea_orm::DatabaseConnection,
    auth: &AuthUser,
) -> Result<UserInfoResp, AppError> {
    let user = user_repo::find_by_id(db, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Biz("用户不存在".into()))?;
    Ok(UserInfoResp::from_model(user, auth))
}

/// 权限码数组（契约 §3.2 的 `/user/access-codes`）：
/// 超管返回 `['super']`；普通用户按角色查按钮权限码（W3 完善）。
pub async fn get_access_codes(
    db: &sea_orm::DatabaseConnection,
    roles: &[String],
) -> anyhow::Result<Vec<String>> {
    if roles.iter().any(|r| r == "super") {
        return Ok(vec!["super".to_string()]);
    }
    // TODO(W3)：sys_role_menu → sys_menu(menu_type=3) 拍平 permission 码，与后端授权点同源
    let _ = db;
    Ok(vec![])
}

/// vben 菜单树（契约 §3.2 的 `/user/menus`）：超管返回全量，普通用户 W3 按角色过滤。
pub async fn get_menus(
    db: &sea_orm::DatabaseConnection,
    roles: &[String],
) -> anyhow::Result<Vec<VbenMenuItem>> {
    if !roles.iter().any(|r| r == "super") {
        // TODO(W3)：按角色过滤 sys_role_menu
        return Ok(vec![]);
    }
    let menus = user_repo::find_all_menus(db).await?;
    Ok(build_menu_tree(menus))
}

/// sys_menu（parent_id 树）→ vben 菜单树：按钮（menu_type=3）不进菜单树，只进权限码。
fn build_menu_tree(menus: Vec<sys_menu::Model>) -> Vec<VbenMenuItem> {
    let nodes: Vec<sys_menu::Model> = menus.into_iter().filter(|m| m.menu_type != 3).collect();
    let mut by_parent: HashMap<u64, Vec<sys_menu::Model>> = HashMap::new();
    for m in nodes {
        by_parent.entry(m.parent_id).or_default().push(m);
    }

    fn build(
        parent_id: u64,
        by_parent: &HashMap<u64, Vec<sys_menu::Model>>,
    ) -> Vec<VbenMenuItem> {
        by_parent
            .get(&parent_id)
            .map(|items| {
                items
                    .iter()
                    .map(|m| VbenMenuItem {
                        path: m.path.clone(),
                        name: m.name.clone(),
                        component: m.component.clone(),
                        meta: VbenMenuMeta {
                            title: m.title.clone(),
                            icon: m.icon.clone(),
                            order: m.sort,
                            keep_alive: m.keep_alive == 1,
                            hide_in_menu: m.hidden == 1,
                        },
                        children: build(m.id, by_parent),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    build(0, &by_parent)
}
