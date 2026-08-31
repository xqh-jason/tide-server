//! 菜单域业务：vben 菜单树构建。

use std::collections::HashMap;

use sea_orm::DatabaseConnection;

use crate::entity::sys_menu;
use crate::modules::menu::dto::{VbenMenuItem, VbenMenuMeta};
use crate::modules::menu::repo as menu_repo;
use crate::modules::permission::SUPER_ROLE_KEY;

/// 菜单树最大深度：防御异常数据（超深 parent 链）导致递归栈溢出。
const MAX_MENU_DEPTH: usize = 10;

/// vben 菜单树（契约 §3.2 的 `/user/menus`）：超管返回全量，普通用户 W3 按角色过滤。
pub async fn get_menus(
    db: &DatabaseConnection,
    roles: &[String],
) -> anyhow::Result<Vec<VbenMenuItem>> {
    if !roles.iter().any(|r| r == SUPER_ROLE_KEY) {
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

    fn build(
        parent_id: u64,
        by_parent: &HashMap<u64, Vec<sys_menu::Model>>,
        depth: usize,
    ) -> Vec<VbenMenuItem> {
        if depth > MAX_MENU_DEPTH {
            return Vec::new();
        }
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
                        children: build(m.id, by_parent, depth + 1),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    build(0, &by_parent, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_menu;
    use chrono::Utc;

    /// 构造单条菜单记录（menu_type=1 目录/页面）。
    fn menu(id: u64, parent_id: u64) -> sys_menu::Model {
        sys_menu::Model {
            id,
            parent_id,
            path: format!("/m{id}"),
            name: format!("m{id}"),
            component: format!("#/views/m{id}.vue"),
            title: format!("M{id}"),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            deleted_at: None,
        }
    }

    /// 超深链（异常数据）不应导致递归栈溢出：超过深度上限的层级被截断。
    #[test]
    fn build_menu_tree_truncates_abnormal_deep_chain() {
        // 60 层单链：1 ← 2 ← ... ← 60（parent 指向上层）
        let menus: Vec<sys_menu::Model> = (1..=60).map(|id| menu(id, id - 1)).collect();

        let tree = build_menu_tree(menus);

        // 第一层只有一个根节点，深度被限制后最深节点的 children 为空
        assert_eq!(tree.len(), 1);
        let mut node = &tree[0];
        let mut depth = 1;
        while !node.children.is_empty() {
            node = &node.children[0];
            depth += 1;
        }
        assert!(
            depth <= MAX_MENU_DEPTH,
            "异常深链应被截断到深度上限，实际深度：{depth}"
        );
    }
}
