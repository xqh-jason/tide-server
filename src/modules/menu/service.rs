//! 菜单域业务：vben 菜单树构建。

use std::collections::HashMap;

use sea_orm::ActiveValue::Set;
use sea_orm::DatabaseConnection;

use crate::entity::sys_menu;
use crate::modules::menu::dto::{
    CreateMenuReq, MenuFilter, MenuListReq, UpdateMenuReq, VbenMenuItem, VbenMenuMeta,
};
use crate::modules::menu::repo as menu_repo;
use crate::modules::permission::SUPER_ROLE_KEY;
use crate::modules::user::repo as user_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 菜单树最大深度：防御异常数据（超深 parent 链）导致递归栈溢出。
const MAX_MENU_DEPTH: usize = 10;

/// vben 菜单树（契约 §3.2 的 `/user/menus`）：超管返回全量，普通用户 W3 按角色过滤。
pub async fn get_menus(
    db: &DatabaseConnection,
    user_id: u64,
) -> Result<Vec<VbenMenuItem>, AppError> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;
    let is_super = roles.iter().any(|r| r.role_key == SUPER_ROLE_KEY);

    let menus = if is_super {
        menu_repo::find_all_menus(db).await?
    } else {
        menu_repo::find_menus_by_user_id(db, user_id).await?
    };
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

/// 菜单分页查询：请求参数（keyword / status / menu_type）组装为 repo 过滤条件。
pub async fn page_menus(
    db: &DatabaseConnection,
    req: &MenuListReq,
) -> Result<PageData<sys_menu::Model>, AppError> {
    let model = menu_repo::find_page(
        db,
        &MenuFilter {
            keyword: req.keyword.clone(),
            status: req.status,
            menu_type: req.menu_type,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(model)
}

/// 创建菜单：name 查重（含软删占位）→ component 格式校验 → 落库（审计字段由 repo 盖章）。
pub async fn create_menu(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreateMenuReq,
) -> Result<sys_menu::Model, AppError> {
    // 检查名称是否存在
    let menu = menu_repo::find_by_name_include_deleted(db, &req.name).await?;
    if let Some(menu) = menu {
        return Err(AppError::Biz(format!("菜单名称已存在：{}", menu.name)));
    }

    let menu_type = req.menu_type.unwrap_or(1);
    // 校验 component 路径
    validate_component(&req.component, menu_type)?;

    // 创建菜单
    let menu = menu_repo::create_menu(
        db,
        sys_menu::ActiveModel {
            parent_id: Set(req.parent_id.unwrap_or(0)),
            path: Set(req.path.clone()),
            name: Set(req.name.clone()),
            component: Set(req.component.clone()),
            title: Set(req.title.clone()),
            icon: Set(req.icon.clone().unwrap_or_default()),
            sort: Set(req.sort.unwrap_or(0)),
            keep_alive: Set(req.keep_alive.unwrap_or(0)),
            hidden: Set(req.hidden.unwrap_or(0)),
            menu_type: Set(menu_type),
            permission: Set(req.permission.clone().unwrap_or_default()),
            status: Set(req.status.unwrap_or(1)),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(menu)
}

/// 更新菜单：判存在 → name 查重排除自身 → component 校验 → 全量覆盖（审计字段由 repo 盖章）。
pub async fn update_menu(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateMenuReq,
) -> Result<sys_menu::Model, AppError> {
    // 检查菜单是否存在（软删视为不存在）
    let Some(_) = menu_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz(format!("菜单不存在：{}", req.id)));
    };

    // 检查名称是否已被其他菜单占用（含软删占位，排除自身）
    if let Some(existing) = menu_repo::find_by_name_include_deleted(db, &req.name).await? {
        if existing.id != req.id {
            return Err(AppError::Biz(format!("菜单名称已存在：{}", existing.name)));
        }
    }

    let menu_type = req.menu_type;
    // 校验 component 路径
    validate_component(&req.component, menu_type)?;

    // 更新菜单
    let menu = menu_repo::update_menu(
        db,
        sys_menu::ActiveModel {
            id: Set(req.id),
            parent_id: Set(req.parent_id),
            path: Set(req.path.clone()),
            name: Set(req.name.clone()),
            component: Set(req.component.clone()),
            title: Set(req.title.clone()),
            icon: Set(req.icon.clone()),
            sort: Set(req.sort),
            keep_alive: Set(req.keep_alive),
            hidden: Set(req.hidden),
            menu_type: Set(menu_type),
            permission: Set(req.permission.clone()),
            status: Set(req.status),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(menu)
}

/// 删除菜单：判存在后软删，repo 层级联软删全部子孙菜单并清空角色关联。
pub async fn delete_menu(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    // 检查菜单是否存在
    let Some(_) = menu_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz("菜单不存在".to_string()));
    };

    menu_repo::soft_delete_menu(db, id).await?;
    Ok(())
}

/// 查询单个菜单详情（排除软删除）；不存在返回业务错误。
pub async fn get_menu(db: &DatabaseConnection, id: u64) -> Result<sys_menu::Model, AppError> {
    let Some(menu) = menu_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("菜单不存在：{id}")));
    };
    Ok(menu)
}

/// 校验 vben component 路径：非按钮必须 `#/views/` 开头且 `.vue` 结尾。
fn validate_component(component: &str, menu_type: i8) -> Result<(), AppError> {
    if menu_type == 3 || component.is_empty() {
        return Ok(());
    }
    if !component.starts_with("#/views/") || !component.ends_with(".vue") {
        return Err(AppError::Biz(
            "component 必须为 #/views/xxx.vue 格式".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_menu, sys_role, sys_role_menu, sys_user, sys_user_role};
    use crate::modules::menu::dto::{CreateMenuReq, UpdateMenuReq};
    use crate::utils::error::AppError;
    use chrono::Local;
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    fn create_req(name: String, component: String) -> CreateMenuReq {
        CreateMenuReq {
            parent_id: Some(0),
            path: format!("/{name}"),
            name,
            component,
            title: unique("title"),
            icon: None,
            sort: Some(0),
            keep_alive: None,
            hidden: None,
            menu_type: Some(1),
            permission: None,
            status: Some(1),
        }
    }

    async fn seed_menu(
        db: &DatabaseConnection,
        name: &str,
        parent_id: u64,
        menu_type: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(parent_id),
            path: Set(format!("/{}", unique("seed_path"))),
            name: Set(name.to_string()),
            component: Set(format!("#/views/{}.vue", unique("seed_comp"))),
            title: Set(unique("seed_title")),
            icon: Set(String::new()),
            sort: Set(0),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(menu_type),
            permission: Set(String::new()),
            status: Set(1),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_user(db: &DatabaseConnection) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique("menu_user")),
            password: Set("x".to_string()),
            nickname: Set("菜单测试用户".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(db: &DatabaseConnection, role_key: &str) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("menu_role")),
            role_key: Set(role_key.to_string()),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 加载全局唯一的 super 角色：已存在则复用（测后不清理），由本测试创建才清理。
    async fn load_super_role(db: &DatabaseConnection) -> (sys_role::Model, bool) {
        let existing = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq(SUPER_ROLE_KEY))
            .one(db)
            .await
            .unwrap();
        if let Some(role) = existing {
            return (role, false);
        }
        let role = seed_role(db, SUPER_ROLE_KEY).await;
        (role, true)
    }

    async fn bind_user_role(db: &DatabaseConnection, user_id: u64, role_id: u64) {
        sys_user_role::ActiveModel {
            user_id: Set(user_id),
            role_id: Set(role_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn bind_role_menu(db: &DatabaseConnection, role_id: u64, menu_id: u64) {
        sys_role_menu::ActiveModel {
            role_id: Set(role_id),
            menu_id: Set(menu_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// 清理顺序：先删关联表，再删主表。
    async fn cleanup(
        db: &DatabaseConnection,
        user_ids: &[u64],
        role_ids: &[u64],
        menu_ids: &[u64],
    ) {
        for user_id in user_ids {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(*user_id))
                .exec(db)
                .await
                .unwrap();
        }
        for role_id in role_ids {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(*role_id))
                .exec(db)
                .await
                .unwrap();
        }
        sys_user::Entity::delete_many()
            .filter(sys_user::Column::Id.is_in(user_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_many()
            .filter(sys_role::Column::Id.is_in(role_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
        sys_menu::Entity::delete_many()
            .filter(sys_menu::Column::Id.is_in(menu_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    /// 创建时 name 重复（含软删占位）应被业务层拒绝。
    #[tokio::test]
    async fn create_menu_rejects_duplicate_name_including_soft_deleted() {
        let db = test_db().await;
        let deleted_name = unique("dup_deleted");
        let live_name = unique("dup_live");
        let deleted = seed_menu(
            &db,
            &deleted_name,
            0,
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let live = seed_menu(&db, &live_name, 0, 1, None).await;

        let result_deleted = create_menu(
            &db,
            ACTOR_ID,
            &create_req(deleted_name, format!("#/views/{}.vue", unique("c"))),
        )
        .await;
        let result_live = create_menu(
            &db,
            ACTOR_ID,
            &create_req(live_name, format!("#/views/{}.vue", unique("c"))),
        )
        .await;

        cleanup(&db, &[], &[], &[deleted.id, live.id]).await;

        assert!(
            matches!(result_deleted, Err(AppError::Biz(_))),
            "软删菜单占用的 name 应被拒绝，实际：{result_deleted:?}"
        );
        assert!(
            matches!(result_live, Err(AppError::Biz(_))),
            "正常菜单占用的 name 应被拒绝，实际：{result_live:?}"
        );
    }

    /// component 必须 `#/views/` 开头且 `.vue` 结尾（vben glob 命中约束）。
    #[tokio::test]
    async fn create_menu_rejects_invalid_component_format() {
        let db = test_db().await;

        let missing_prefix = create_menu(
            &db,
            ACTOR_ID,
            &create_req(unique("bad_prefix"), "views/foo.vue".to_string()),
        )
        .await;
        let missing_suffix = create_menu(
            &db,
            ACTOR_ID,
            &create_req(unique("bad_suffix"), "#/views/foo".to_string()),
        )
        .await;

        assert!(
            matches!(missing_prefix, Err(AppError::Biz(_))),
            "component 缺少 #/views/ 前缀应被拒绝，实际：{missing_prefix:?}"
        );
        assert!(
            matches!(missing_suffix, Err(AppError::Biz(_))),
            "component 缺少 .vue 后缀应被拒绝，实际：{missing_suffix:?}"
        );
    }

    /// 更新时 name 与他人重复应被拒绝，但保留自身 name 不算重复。
    #[tokio::test]
    async fn update_menu_rejects_duplicate_name_excluding_self() {
        let db = test_db().await;
        let name_a = unique("menu_a");
        let name_b = unique("menu_b");
        let menu_a = seed_menu(&db, &name_a, 0, 1, None).await;
        let menu_b = seed_menu(&db, &name_b, 0, 1, None).await;

        let update_req = |name: String| UpdateMenuReq {
            id: menu_b.id,
            parent_id: 0,
            path: format!("/{name}"),
            name,
            component: format!("#/views/{}.vue", unique("comp")),
            title: unique("title"),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
        };

        let dup = update_menu(&db, ACTOR_ID, &update_req(name_a.clone())).await;
        let keep_self = update_menu(&db, ACTOR_ID, &update_req(name_b.clone())).await;

        cleanup(&db, &[], &[], &[menu_a.id, menu_b.id]).await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 name 应返回 Biz 业务错误，实际：{dup:?}"
        );
        let updated = keep_self.expect("保留自身 name 应更新成功");
        assert_eq!(updated.name, name_b);
    }

    /// 更新不存在的菜单（或已软删菜单）应返回业务错误。
    #[tokio::test]
    async fn update_menu_returns_biz_error_when_menu_missing() {
        let db = test_db().await;

        let missing = update_menu(
            &db,
            ACTOR_ID,
            &UpdateMenuReq {
                id: 9_999_999_999,
                parent_id: 0,
                path: "/missing".to_string(),
                name: unique("missing"),
                component: format!("#/views/{}.vue", unique("comp")),
                title: unique("title"),
                icon: String::new(),
                sort: 0,
                keep_alive: 0,
                hidden: 0,
                menu_type: 1,
                permission: String::new(),
                status: 1,
            },
        )
        .await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "更新不存在的菜单应返回 Biz 业务错误，实际：{missing:?}"
        );

        let deleted_name = unique("deleted_menu");
        let deleted = seed_menu(
            &db,
            &deleted_name,
            0,
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let update_deleted = update_menu(
            &db,
            ACTOR_ID,
            &UpdateMenuReq {
                id: deleted.id,
                parent_id: 0,
                path: "/deleted".to_string(),
                name: deleted_name,
                component: format!("#/views/{}.vue", unique("comp")),
                title: unique("title"),
                icon: String::new(),
                sort: 0,
                keep_alive: 0,
                hidden: 0,
                menu_type: 1,
                permission: String::new(),
                status: 1,
            },
        )
        .await;
        cleanup(&db, &[], &[], &[deleted.id]).await;
        assert!(
            matches!(update_deleted, Err(AppError::Biz(_))),
            "更新已软删菜单应返回 Biz 业务错误，实际：{update_deleted:?}"
        );
    }

    /// 删除不存在的菜单应返回业务错误。
    #[tokio::test]
    async fn delete_menu_returns_biz_error_when_menu_missing() {
        let db = test_db().await;

        let missing = delete_menu(&db, 9_999_999_999).await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "删除不存在的菜单应返回 Biz 业务错误，实际：{missing:?}"
        );
    }

    /// 普通用户只看到其角色绑定的菜单（父子结构完整），未绑定菜单不出现。
    #[tokio::test]
    async fn get_menus_regular_user_filters_by_role_bindings() {
        let db = test_db().await;
        let user = seed_user(&db).await;
        let role = seed_role(&db, &unique("regular_key")).await;
        bind_user_role(&db, user.id, role.id).await;
        let parent = seed_menu(&db, &unique("bind_parent"), 0, 1, None).await;
        let child = seed_menu(&db, &unique("bind_child"), parent.id, 1, None).await;
        let other = seed_menu(&db, &unique("unbound"), 0, 1, None).await;
        bind_role_menu(&db, role.id, parent.id).await;
        bind_role_menu(&db, role.id, child.id).await;

        let tree = get_menus(&db, user.id).await.unwrap();

        cleanup(
            &db,
            &[user.id],
            &[role.id],
            &[parent.id, child.id, other.id],
        )
        .await;

        assert_eq!(tree.len(), 1, "普通用户只应看到绑定的顶级菜单");
        assert_eq!(tree[0].name, parent.name);
        assert_eq!(tree[0].children.len(), 1, "绑定子菜单应保留层级");
        assert_eq!(tree[0].children[0].name, child.name);
    }

    /// 超管返回全量菜单树（未绑定也可见），按钮（menu_type=3）不进树。
    #[tokio::test]
    async fn get_menus_super_returns_full_tree_without_buttons() {
        let db = test_db().await;
        let user = seed_user(&db).await;
        let (super_role, owned_by_test) = load_super_role(&db).await;
        bind_user_role(&db, user.id, super_role.id).await;
        let page = seed_menu(&db, &unique("super_page"), 0, 1, None).await;
        let button = seed_menu(&db, &unique("super_button"), page.id, 3, None).await;

        let tree = get_menus(&db, user.id).await.unwrap();

        let role_ids: &[u64] = if owned_by_test { &[super_role.id] } else { &[] };
        cleanup(&db, &[user.id], role_ids, &[page.id, button.id]).await;

        assert!(
            contains_name(&tree, &page.name),
            "超管树应包含创建的页面（全量菜单）"
        );
        assert!(
            !contains_name(&tree, &button.name),
            "按钮（menu_type=3）不应进入菜单树"
        );
    }

    /// 递归判断菜单树中是否存在指定 name 的节点。
    fn contains_name(nodes: &[VbenMenuItem], name: &str) -> bool {
        nodes
            .iter()
            .any(|n| n.name == name || contains_name(&n.children, name))
    }

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
            created_at: Local::now().naive_local(),
            updated_at: Local::now().naive_local(),
            created_by: None,
            updated_by: None,
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
