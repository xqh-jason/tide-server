//! 菜单域业务：vben 菜单树构建。

use std::collections::HashMap;

use sea_orm::DatabaseConnection;

use crate::entity::sys_menu;
use crate::modules::menu::dto::{VbenMenuItem, VbenMenuMeta};
use crate::modules::menu::repo as menu_repo;

/// vben 菜单树（契约 §3.2 的 `/user/menus`）：超管返回全量，普通用户 W3 按角色过滤。
pub async fn get_menus(
    db: &DatabaseConnection,
    roles: &[String],
) -> anyhow::Result<Vec<VbenMenuItem>> {
    if !roles.iter().any(|r| r == "super") {
        // TODO(W3)：按角色过滤 sys_role_menu
        return Ok(vec![]);
    }
    let menus = menu_repo::find_all_menus(db).await?;
    Ok(build_menu_tree(menus))
}

/// sys_menu（parent_id 树）→ vben 菜单树：按钮（menu_type=3）不进菜单树，只进权限码。
fn build_menu_tree(menus: Vec<sys_menu::Model>) -> Vec<VbenMenuItem> {
    let nodes: Vec<sys_menu::Model> = menus.into_iter().filter(|m| m.menu_type != 3).collect();
    let mut by_parent: HashMap<u64, Vec<sys_menu::Model>> = HashMap::new();
    for m in nodes {
        by_parent.entry(m.parent_id).or_default().push(m);
    }

    fn build(parent_id: u64, by_parent: &HashMap<u64, Vec<sys_menu::Model>>) -> Vec<VbenMenuItem> {
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
