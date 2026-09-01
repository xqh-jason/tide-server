use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, TransactionTrait};

use crate::entity::{sys_role, sys_role_api, sys_role_menu};
use crate::modules::role::dto::RoleFilter;

/// 查询单个有效角色（排除软删除）。
pub async fn find_by_id(
    db: &DatabaseConnection,
    id: u64,
) -> anyhow::Result<Option<sys_role::Model>> {
    let role = sys_role::Entity::find()
        .filter(sys_role::Column::Id.eq(id))
        .filter(sys_role::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(role)
}

/// 查重辅助：role_key 唯一（含软删占位）。
pub async fn find_by_role_key_include_deleted(
    db: &DatabaseConnection,
    role_key: &str,
) -> anyhow::Result<Option<sys_role::Model>> {
    let role = sys_role::Entity::find()
        .filter(sys_role::Column::RoleKey.eq(role_key))
        .one(db)
        .await?;
    Ok(role)
}

/// 查重辅助：role_name 唯一（含软删占位）。
pub async fn find_by_role_name_include_deleted(
    db: &DatabaseConnection,
    role_name: &str,
) -> anyhow::Result<Option<sys_role::Model>> {
    let role = sys_role::Entity::find()
        .filter(sys_role::Column::RoleName.eq(role_name))
        .one(db)
        .await?;
    Ok(role)
}

/// 分页 + 动态过滤查询角色（keyword 模糊匹配 role_name/role_key，status 精确）。
pub async fn find_page(
    db: &DatabaseConnection,
    filter: &RoleFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<sys_role::Model>> {
    let mut cond = Condition::all();

    if let Some(status) = filter.status {
        cond = cond.add(sys_role::Column::Status.eq(status));
    }
    if let Some(kw) = &filter.keyword {
        // keyword 命中角色名或角色键任一即可（OR 语义）
        let kw_cond = Condition::any()
            .add(sys_role::Column::RoleName.like(format!("%{kw}%")))
            .add(sys_role::Column::RoleKey.like(format!("%{kw}%")));
        cond = cond.add(kw_cond);
    }

    let select = sys_role::Entity::find()
        .filter(cond)
        .filter(sys_role::Column::DeletedAt.is_null());
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 事务写入角色并维护菜单/API 关联（关系表硬删除，只插不判软删）。
pub async fn create_role_with_links(
    db: &DatabaseConnection,
    role: sys_role::ActiveModel,
    menu_ids: Vec<u64>,
    api_ids: Vec<u64>,
) -> anyhow::Result<sys_role::Model> {
    let txn = db.begin().await?;
    let role = role.insert(&txn).await?;
    // 插入菜单关联
    if !menu_ids.is_empty() {
        let role_menu_ids = menu_ids
            .into_iter()
            .map(|menu_id| sys_role_menu::ActiveModel {
                role_id: Set(role.id),
                menu_id: Set(menu_id),
            })
            .collect::<Vec<_>>();
        sys_role_menu::Entity::insert_many(role_menu_ids)
            .exec(&txn)
            .await?;
    }
    // 插入 API关联
    if !api_ids.is_empty() {
        let role_api_ids = api_ids
            .into_iter()
            .map(|api_id| sys_role_api::ActiveModel {
                role_id: Set(role.id),
                api_id: Set(api_id),
            })
            .collect::<Vec<_>>();
        sys_role_api::Entity::insert_many(role_api_ids)
            .exec(&txn)
            .await?;
    }

    txn.commit().await?;
    Ok(role)
}

/// 事务更新角色并重建菜单/API 关联（先删旧关联，再插新关联）。
pub async fn update_role_with_links(
    db: &DatabaseConnection,
    role: sys_role::ActiveModel,
    menu_ids: Vec<u64>,
    api_ids: Vec<u64>,
) -> anyhow::Result<sys_role::Model> {
    let txn = db.begin().await?;

    // 更新角色
    let role = role.update(&txn).await?;

    // 删除旧菜单关联
    sys_role_menu::Entity::delete_many()
        .filter(sys_role_menu::Column::RoleId.eq(role.id))
        .exec(&txn)
        .await?;

    // 删除旧 API关联
    sys_role_api::Entity::delete_many()
        .filter(sys_role_api::Column::RoleId.eq(role.id))
        .exec(&txn)
        .await?;

    // 写入菜单关联
    if !menu_ids.is_empty() {
        let role_menu_ids = menu_ids
            .into_iter()
            .map(|menu_id| sys_role_menu::ActiveModel {
                role_id: Set(role.id),
                menu_id: Set(menu_id),
            })
            .collect::<Vec<_>>();

        sys_role_menu::Entity::insert_many(role_menu_ids)
            .exec(&txn)
            .await?;
    }

    // 写入 API关联
    if !api_ids.is_empty() {
        let role_api_ids = api_ids
            .into_iter()
            .map(|api_id| sys_role_api::ActiveModel {
                role_id: Set(role.id),
                api_id: Set(api_id),
            })
            .collect::<Vec<_>>();
        sys_role_api::Entity::insert_many(role_api_ids)
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(role)
}

/// 软删除角色：同一事务内物理清空关联（sys_role_menu / sys_role_api），
/// 再把 `sys_role.deleted_at` 置为当前时间。
pub async fn soft_delete_role(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    let txn = db.begin().await?;
    // 删除旧菜单关联
    sys_role_menu::Entity::delete_many()
        .filter(sys_role_menu::Column::RoleId.eq(id))
        .exec(&txn)
        .await?;
    // 删除旧 API关联
    sys_role_api::Entity::delete_many()
        .filter(sys_role_api::Column::RoleId.eq(id))
        .exec(&txn)
        .await?;
    // 更新角色
    let role = sys_role::Entity::find()
        .filter(sys_role::Column::Id.eq(id))
        .one(db)
        .await?;
    if let Some(role) = role {
        let mut role: sys_role::ActiveModel = role.into();
        role.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        role.update(&txn).await?;
    }

    txn.commit().await?;
    Ok(true)
}

pub async fn find_by_ids(
    db: &DatabaseConnection,
    ids: Vec<u64>,
) -> anyhow::Result<Vec<sys_role::Model>> {
    let roles = sys_role::Entity::find()
        .filter(sys_role::Column::Id.is_in(ids))
        .filter(sys_role::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_menu, sys_role_api, sys_role_menu};
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn seed_role(
        db: &DatabaseConnection,
        role_name: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(role_name.to_string()),
            role_key: Set(unique("role_key")),
            sort: Set(0),
            status: Set(status),
            remark: Set(String::new()),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_menu(db: &DatabaseConnection) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(0),
            title: Set(unique("role_menu")),
            name: Set(unique("RoleMenu")),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_api(db: &DatabaseConnection) -> sys_api::Model {
        sys_api::ActiveModel {
            path: Set(format!("/api/{}", unique("role_api"))),
            method: Set("POST".to_string()),
            description: Set("角色域测试接口".to_string()),
            api_group: Set("role".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 清理顺序：先删关联，再删角色主表，最后删菜单/API（关系表硬删除）。
    async fn cleanup(db: &DatabaseConnection, role_ids: &[u64], menu_ids: &[u64], api_ids: &[u64]) {
        for role_id in role_ids {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(*role_id))
                .exec(db)
                .await
                .unwrap();
            sys_role_api::Entity::delete_many()
                .filter(sys_role_api::Column::RoleId.eq(*role_id))
                .exec(db)
                .await
                .unwrap();
            sys_role::Entity::delete_by_id(*role_id)
                .exec(db)
                .await
                .unwrap();
        }
        for menu_id in menu_ids {
            sys_menu::Entity::delete_by_id(*menu_id)
                .exec(db)
                .await
                .unwrap();
        }
        for api_id in api_ids {
            sys_api::Entity::delete_by_id(*api_id)
                .exec(db)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn create_role_with_links_inserts_role_and_links() {
        let db = test_db().await;
        let menu_a = seed_menu(&db).await;
        let menu_b = seed_menu(&db).await;
        let api_a = seed_api(&db).await;
        let api_b = seed_api(&db).await;
        let role_name = unique("create_link_role");
        let role = sys_role::ActiveModel {
            role_name: Set(role_name.clone()),
            role_key: Set(unique("create_link_key")),
            sort: Set(1),
            status: Set(1),
            remark: Set("关联测试".to_string()),
            ..Default::default()
        };

        let created = create_role_with_links(
            &db,
            role,
            vec![menu_a.id, menu_b.id],
            vec![api_a.id, api_b.id],
        )
        .await
        .unwrap();

        let menu_links = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        let api_links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(
            &db,
            &[created.id],
            &[menu_a.id, menu_b.id],
            &[api_a.id, api_b.id],
        )
        .await;

        assert_eq!(created.role_name, role_name);
        assert_eq!(menu_links.len(), 2);
        assert_eq!(api_links.len(), 2);
    }

    #[tokio::test]
    async fn create_role_with_links_supports_empty_link_lists() {
        let db = test_db().await;
        let role_name = unique("create_empty_role");
        let role = sys_role::ActiveModel {
            role_name: Set(role_name.clone()),
            role_key: Set(unique("create_empty_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        };

        let created = create_role_with_links(&db, role, vec![], vec![])
            .await
            .unwrap();

        let menu_links = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        let api_links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(&db, &[created.id], &[], &[]).await;

        assert_eq!(created.role_name, role_name);
        assert!(menu_links.is_empty(), "空菜单列表不应创建关联");
        assert!(api_links.is_empty(), "空 API 列表不应创建关联");
    }

    #[tokio::test]
    async fn update_role_with_links_rebuilds_links_in_transaction() {
        let db = test_db().await;
        let old_menu = seed_menu(&db).await;
        let new_menu = seed_menu(&db).await;
        let old_api = seed_api(&db).await;
        let new_api = seed_api(&db).await;
        let role_name = unique("update_link_role");
        let created = create_role_with_links(
            &db,
            sys_role::ActiveModel {
                role_name: Set(role_name.clone()),
                role_key: Set(unique("update_link_key")),
                sort: Set(0),
                status: Set(1),
                remark: Set(String::new()),
                ..Default::default()
            },
            vec![old_menu.id],
            vec![old_api.id],
        )
        .await
        .unwrap();

        // 更新：改名 + 全量替换关联（旧菜单/旧 API 清掉，换成新菜单/新 API）。
        let updated = update_role_with_links(
            &db,
            sys_role::ActiveModel {
                id: Set(created.id),
                role_name: Set(format!("{role_name}_v2")),
                role_key: Set(created.role_key.clone()),
                ..Default::default()
            },
            vec![new_menu.id],
            vec![new_api.id],
        )
        .await
        .unwrap();

        let menu_links = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        let api_links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(
            &db,
            &[created.id],
            &[old_menu.id, new_menu.id],
            &[old_api.id, new_api.id],
        )
        .await;

        assert_eq!(updated.role_name, format!("{role_name}_v2"));
        assert_eq!(menu_links.len(), 1);
        assert_eq!(menu_links[0].menu_id, new_menu.id, "旧菜单关联应被清空");
        assert_eq!(api_links.len(), 1);
        assert_eq!(api_links[0].api_id, new_api.id, "旧 API 关联应被清空");
    }

    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted_role() {
        let db = test_db().await;
        let live = seed_role(&db, &unique("find_live"), 1, None).await;
        let disabled = seed_role(&db, &unique("find_disabled"), 0, None).await;
        let deleted = seed_role(
            &db,
            &unique("find_deleted"),
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_disabled = find_by_id(&db, disabled.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        cleanup(&db, &[live.id, disabled.id, deleted.id], &[], &[]).await;

        assert_eq!(found_live.as_ref().map(|r| r.id), Some(live.id));
        assert_eq!(
            found_disabled.as_ref().map(|r| r.id),
            Some(disabled.id),
            "禁用角色仍应被 find_by_id 查到（get/update 场景禁用不等于不存在）"
        );
        assert!(found_deleted.is_none(), "软删除角色不应被 find_by_id 查到");
    }

    #[tokio::test]
    async fn find_page_filters_by_keyword_and_status_excludes_deleted() {
        let db = test_db().await;
        let keyword = unique("page_keyword");
        let live = seed_role(&db, &format!("{keyword}_live"), 1, None).await;
        let disabled = seed_role(&db, &format!("{keyword}_disabled"), 0, None).await;
        let deleted = seed_role(
            &db,
            &format!("{keyword}_deleted"),
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;

        let data_all = find_page(
            &db,
            &RoleFilter {
                keyword: Some(keyword.clone()),
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let data_enabled = find_page(
            &db,
            &RoleFilter {
                keyword: Some(keyword.clone()),
                status: Some(1),
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[live.id, disabled.id, deleted.id], &[], &[]).await;

        assert_eq!(data_all.total, 2, "软删除角色不应进入分页");
        assert_eq!(data_all.items.len(), 2);
        assert_eq!(data_enabled.total, 1, "status 过滤应只保留启用角色");
        assert_eq!(data_enabled.items[0].id, live.id);
    }

    #[tokio::test]
    async fn soft_delete_role_removes_links_and_excludes_role() {
        let db = test_db().await;
        let menu = seed_menu(&db).await;
        let api = seed_api(&db).await;
        let role_name = unique("soft_delete_role");
        let created = create_role_with_links(
            &db,
            sys_role::ActiveModel {
                role_name: Set(role_name.clone()),
                role_key: Set(unique("soft_delete_key")),
                sort: Set(0),
                status: Set(1),
                remark: Set(String::new()),
                ..Default::default()
            },
            vec![menu.id],
            vec![api.id],
        )
        .await
        .unwrap();

        let deleted = soft_delete_role(&db, created.id).await.unwrap();

        let menu_links = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        let api_links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        let by_id = find_by_id(&db, created.id).await.unwrap();

        cleanup(&db, &[created.id], &[menu.id], &[api.id]).await;

        assert!(deleted);
        assert!(menu_links.is_empty(), "软删角色应物理清空菜单关联");
        assert!(api_links.is_empty(), "软删角色应物理清空 API 关联");
        assert!(by_id.is_none(), "软删角色不应再被查询到");
    }

    #[tokio::test]
    async fn find_by_ids_excludes_deleted_roles() {
        let db = test_db().await;

        let live_role = sys_role::ActiveModel {
            role_name: Set("正常角色".to_string()),
            role_key: Set(unique("live_role")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let deleted_role = sys_role::ActiveModel {
            role_name: Set("已删除角色".to_string()),
            role_key: Set(unique("deleted_role")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut mark_deleted: sys_role::ActiveModel = deleted_role.clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
        mark_deleted.update(&db).await.unwrap();

        let found = find_by_ids(&db, vec![live_role.id, deleted_role.id]).await;

        sys_role::Entity::delete_by_id(live_role.id)
            .exec(&db)
            .await
            .unwrap();
        sys_role::Entity::delete_by_id(deleted_role.id)
            .exec(&db)
            .await
            .unwrap();

        let found = found.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, live_role.id);
    }
}
