//! 菜单域数据访问。

use crate::entity::sys_role_menu;
use crate::entity::{sys_menu, sys_menu::Model};
use crate::modules::menu::dto::MenuFilter;
use crate::modules::user::repo as user_repo;
use crate::utils::PageData;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, QueryOrder, QuerySelect, TransactionTrait};

/// 查询全部启用菜单（W2 超管全量菜单树；按角色过滤 W3 再做）。
pub async fn find_all_menus(db: &DatabaseConnection) -> anyhow::Result<Vec<Model>> {
    Ok(sys_menu::Entity::find()
        .filter(sys_menu::Column::Status.eq(1))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .order_by_asc(sys_menu::Column::Sort)
        .all(db)
        .await?)
}

pub async fn find_page(
    db: &DatabaseConnection,
    filter: &MenuFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = &filter.keyword {
        cond = cond.add(sys_menu::Column::Name.like(format!("%{}%", keyword)));
    }

    if let Some(status) = filter.status {
        cond = cond.add(sys_menu::Column::Status.eq(status));
    }

    if let Some(menu_type) = filter.menu_type {
        cond = cond.add(sys_menu::Column::MenuType.eq(menu_type));
    }

    let select = sys_menu::Entity::find()
        .filter(cond)
        .filter(sys_menu::Column::DeletedAt.is_null());

    crate::utils::paginate(select, db, page_index, page_size).await
}

pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    let menu = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.eq(id))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(menu)
}

pub async fn create_menu(
    db: &DatabaseConnection,
    model: sys_menu::ActiveModel,
) -> anyhow::Result<Model> {
    let model = model.insert(db).await?;
    Ok(model)
}

pub async fn update_menu(
    db: &DatabaseConnection,
    model: sys_menu::ActiveModel,
) -> anyhow::Result<Model> {
    let model = model.update(db).await?;
    Ok(model)
}

/// 软删除菜单：递归收集目标及其全部子孙菜单（BFS），在同一事务内
/// 清空这些菜单的角色关联（`sys_role_menu` 硬删除）并把主表 `deleted_at` 置为当前时间。
///
/// 子级收集**不过滤软删状态**：即使某个中间节点已软删，其下仍正常的子孙也要级联处理，
/// 避免父已删、子孤立的脏数据。
pub async fn soft_delete_menu(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    // 目标菜单必须存在（排除软删），否则视为无可删除
    let Some(menu) = find_by_id(db, id).await? else {
        return Ok(false);
    };

    // BFS 收集目标 + 全部子孙 id
    let mut ids = vec![menu.id];
    let mut frontier = vec![menu.id];
    while !frontier.is_empty() {
        let children = sys_menu::Entity::find()
            .filter(sys_menu::Column::ParentId.is_in(frontier))
            .column(sys_menu::Column::Id)
            .all(db)
            .await?;
        let child_ids: Vec<u64> = children.into_iter().map(|c| c.id).collect();
        if child_ids.is_empty() {
            break;
        }
        frontier = child_ids.clone();
        ids.extend(child_ids);
    }

    let now = chrono::Local::now().naive_local();
    let txn = db.begin().await?;
    // 关系表硬删除：清掉这些菜单的角色绑定
    sys_role_menu::Entity::delete_many()
        .filter(sys_role_menu::Column::MenuId.is_in(ids.clone()))
        .exec(&txn)
        .await?;
    // 主表批量软删（含目标与全部子孙）
    sys_menu::Entity::update_many()
        .col_expr(
            sys_menu::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(Some(now)),
        )
        .filter(sys_menu::Column::Id.is_in(ids))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(true)
}

pub async fn find_by_name_include_deleted(
    db: &DatabaseConnection,
    name: &str,
) -> anyhow::Result<Option<Model>> {
    let menu = sys_menu::Entity::find()
        .filter(sys_menu::Column::Name.eq(name))
        .one(db)
        .await?;
    Ok(menu)
}

pub async fn find_menus_by_user_id(
    db: &DatabaseConnection,
    user_id: u64,
) -> anyhow::Result<Vec<Model>> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;
    let role_ids = roles.into_iter().map(|r| r.id).collect::<Vec<_>>();
    if role_ids.is_empty() {
        return Ok(vec![]);
    }

    let menu_ids = sys_role_menu::Entity::find()
        .filter(sys_role_menu::Column::RoleId.is_in(role_ids))
        .column(sys_role_menu::Column::MenuId)
        .all(db)
        .await?;
    let menu_ids = menu_ids.into_iter().map(|m| m.menu_id).collect::<Vec<_>>();
    if menu_ids.is_empty() {
        return Ok(vec![]);
    }

    let menus = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.is_in(menu_ids))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .filter(sys_menu::Column::Status.eq(1))
        .all(db)
        .await?;
    Ok(menus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_menu;
    use crate::modules::menu::dto::MenuFilter;
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

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

    async fn seed_menu(
        db: &DatabaseConnection,
        name: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(0),
            path: Set(format!("/{}", unique("seed_path"))),
            name: Set(name.to_string()),
            component: Set(format!("#/views/{}.vue", unique("seed_comp"))),
            title: Set(unique("seed_title")),
            icon: Set(String::new()),
            sort: Set(0),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_menu::Entity::delete_many()
            .filter(sys_menu::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    /// 创建菜单：ActiveModel 显式字段按值落库。
    #[tokio::test]
    async fn create_menu_persists_fields_and_defaults() {
        let db = test_db().await;
        let name = unique("create_menu");
        let model = sys_menu::ActiveModel {
            parent_id: Set(0),
            path: Set(format!("/{name}")),
            name: Set(name.clone()),
            component: Set(format!("#/views/{name}.vue")),
            title: Set(format!("T{name}")),
            icon: Set("mdi:test".to_string()),
            sort: Set(5),
            keep_alive: Set(0),
            hidden: Set(1),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        };

        let created = create_menu(&db, model).await.unwrap();

        cleanup(&db, &[created.id]).await;

        assert_eq!(created.name, name);
        assert_eq!(created.parent_id, 0);
        assert_eq!(created.icon, "mdi:test");
        assert_eq!(created.sort, 5);
        assert_eq!(created.hidden, 1);
        assert_eq!(created.keep_alive, 0);
        assert_eq!(created.menu_type, 1);
        assert_eq!(created.status, 1);
        assert_eq!(created.permission, "");
    }

    /// find_by_id 应排除软删除菜单。
    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted_menu() {
        let db = test_db().await;
        let live = seed_menu(&db, &unique("find_live"), 1, None).await;
        let deleted = seed_menu(
            &db,
            &unique("find_deleted"),
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        cleanup(&db, &[live.id, deleted.id]).await;

        assert_eq!(found_live.as_ref().map(|m| m.id), Some(live.id));
        assert!(found_deleted.is_none(), "软删除菜单不应被 find_by_id 查到");
    }

    /// 分页：keyword 命中 title/name/path，status/menu_type 精确过滤，排除软删。
    #[tokio::test]
    async fn find_page_filters_by_keyword_status_and_menu_type_excludes_deleted() {
        let db = test_db().await;
        let keyword = unique("page_keyword");
        let live = seed_menu(&db, &format!("{keyword}_live"), 1, None).await;
        let disabled = seed_menu(&db, &format!("{keyword}_disabled"), 0, None).await;
        let deleted = seed_menu(
            &db,
            &format!("{keyword}_deleted"),
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;

        let data_all = find_page(
            &db,
            &MenuFilter {
                keyword: Some(keyword.clone()),
                status: None,
                menu_type: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let data_enabled = find_page(
            &db,
            &MenuFilter {
                keyword: Some(keyword.clone()),
                status: Some(1),
                menu_type: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let data_buttons = find_page(
            &db,
            &MenuFilter {
                keyword: Some(keyword.clone()),
                status: None,
                menu_type: Some(3),
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[live.id, disabled.id, deleted.id]).await;

        assert_eq!(data_all.total, 2, "软删除菜单不应进入分页");
        assert_eq!(data_enabled.total, 1, "status 过滤应只保留启用菜单");
        assert_eq!(data_buttons.total, 0, "menu_type 过滤应匹配按钮类型");
    }

    /// 软删除：deleted_at 被置为当前时间，且随后 find_by_id 查不到。
    #[tokio::test]
    async fn soft_delete_menu_sets_deleted_at() {
        let db = test_db().await;
        let menu = seed_menu(&db, &unique("soft_delete"), 1, None).await;

        let deleted = soft_delete_menu(&db, menu.id).await.unwrap();
        let after = find_by_id(&db, menu.id).await.unwrap();

        cleanup(&db, &[menu.id]).await;

        assert!(deleted);
        assert!(after.is_none(), "软删后 find_by_id 不应查到菜单");
    }

    /// 级联软删：删除父菜单时，所有子孙菜单一起软删（BFS 递归）。
    #[tokio::test]
    async fn soft_delete_menu_cascades_to_all_descendants() {
        let db = test_db().await;
        let parent = seed_menu(&db, &unique("cascade_parent"), 1, None).await;
        let child = sys_menu::ActiveModel {
            parent_id: Set(parent.id),
            path: Set(format!("/{}", unique("child_path"))),
            name: Set(unique("cascade_child")),
            component: Set(format!("#/views/{}.vue", unique("child_comp"))),
            title: Set(unique("child_title")),
            icon: Set(String::new()),
            sort: Set(1),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let grandchild = sys_menu::ActiveModel {
            parent_id: Set(child.id),
            path: Set(format!("/{}", unique("grand_path"))),
            name: Set(unique("cascade_grandchild")),
            component: Set(format!("#/views/{}.vue", unique("grand_comp"))),
            title: Set(unique("grand_title")),
            icon: Set(String::new()),
            sort: Set(2),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let deleted = soft_delete_menu(&db, parent.id).await.unwrap();
        let parent_after = find_by_id(&db, parent.id).await.unwrap();
        let child_after = find_by_id(&db, child.id).await.unwrap();
        let grandchild_after = find_by_id(&db, grandchild.id).await.unwrap();

        cleanup(&db, &[parent.id, child.id, grandchild.id]).await;

        assert!(deleted);
        assert!(parent_after.is_none(), "父菜单应被软删");
        assert!(child_after.is_none(), "子菜单应被级联软删");
        assert!(grandchild_after.is_none(), "孙菜单应被级联软删");
    }

    /// 更新菜单：全量覆盖主表字段（与角色域全量更新语义一致）。
    #[tokio::test]
    async fn update_menu_overwrites_fields() {
        let db = test_db().await;
        let menu = seed_menu(&db, &unique("update_before"), 1, None).await;
        let new_name = unique("update_after");

        let mut model: sys_menu::ActiveModel = menu.clone().into();
        model.path = Set(format!("/{new_name}"));
        model.name = Set(new_name.clone());
        model.component = Set(format!("#/views/{new_name}.vue"));
        model.title = Set(format!("T{new_name}"));
        model.icon = Set("mdi:updated".to_string());
        model.sort = Set(9);
        model.keep_alive = Set(1);
        model.hidden = Set(1);
        model.status = Set(1);

        let updated = update_menu(&db, model).await.unwrap();

        cleanup(&db, &[menu.id]).await;

        assert_eq!(updated.name, new_name);
        assert_eq!(updated.title, format!("T{new_name}"));
        assert_eq!(updated.icon, "mdi:updated");
        assert_eq!(updated.sort, 9);
        assert_eq!(updated.keep_alive, 1);
    }

    /// 查重辅助：包含软删除记录（唯一索引占位检查用）。
    #[tokio::test]
    async fn find_by_name_include_deleted_finds_soft_deleted_menu() {
        let db = test_db().await;
        let name = unique("dup_name");
        let deleted = seed_menu(&db, &name, 1, Some(chrono::Utc::now().naive_utc())).await;

        let found = find_by_name_include_deleted(&db, &name).await.unwrap();

        cleanup(&db, &[deleted.id]).await;

        assert_eq!(found.as_ref().map(|m| m.id), Some(deleted.id));
    }
}
