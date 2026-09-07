use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, DatabaseConnection, TransactionTrait};

use crate::entity::{sys_role, sys_role_api, sys_role_menu};
use crate::modules::role::dto::RoleFilter;

/// 查询单个有效角色（排除软删除）。
pub async fn find_by_id(
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
    role_name: &str,
) -> anyhow::Result<Option<sys_role::Model>> {
    let role = sys_role::Entity::find()
        .filter(sys_role::Column::RoleName.eq(role_name))
        .one(db)
        .await?;
    Ok(role)
}

/// 分页 + 动态过滤查询角色（keyword 模糊匹配 role_name/role_key，status 精确，
/// created_by/updated_by/时间范围审计过滤）。
pub async fn find_page(
    db: &impl ConnectionTrait,
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
    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_role::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_role::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_role::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_role::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_role::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_role::Column::UpdatedAt.lte(v));
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
    actor_id: u64,
) -> anyhow::Result<sys_role::Model> {
    let txn = db.begin().await?;
    // 审计字段由 repo 统一盖章：create 时创建人与更新人同源
    let mut role = role;
    role.created_by = Set(actor_id);
    role.updated_by = Set(actor_id);
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
    actor_id: u64,
) -> anyhow::Result<sys_role::Model> {
    let txn = db.begin().await?;

    // 审计字段由 repo 统一盖章：只刷新更新人，created_by 保持 NotSet 不被覆盖
    let mut role = role;
    role.updated_by = Set(actor_id);

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

/// 批量按 id 查询有效角色（排除软删除）；无匹配时返回空数组。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: Vec<u64>,
) -> anyhow::Result<Vec<sys_role::Model>> {
    let roles = sys_role::Entity::find()
        .filter(sys_role::Column::Id.is_in(ids))
        .filter(sys_role::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(roles)
}

/// 通用更新（ActiveModel 入参，状态更新等单字段场景用）。
pub async fn update_role(
    db: &impl ConnectionTrait,
    role: sys_role::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let mut role = role;
    role.updated_by = Set(actor_id);
    role.update(db).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_menu, sys_role_api, sys_role_menu};
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn seed_role(
        db: &impl ConnectionTrait,
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

    async fn seed_menu(db: &impl ConnectionTrait) -> sys_menu::Model {
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

    async fn seed_api(db: &impl ConnectionTrait) -> sys_api::Model {
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
    async fn cleanup(
        db: &impl ConnectionTrait,
        role_ids: &[u64],
        menu_ids: &[u64],
        api_ids: &[u64],
    ) {
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
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
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
            ACTOR_ID,
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
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
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

        let created = create_role_with_links(&db, role, vec![], vec![], ACTOR_ID)
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
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
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
            ACTOR_ID,
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
            ACTOR_ID,
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
        let db = test_txn().await;
        let live = seed_role(&db, &unique("find_live"), 1, None).await;
        let disabled = seed_role(&db, &unique("find_disabled"), 0, None).await;
        let deleted = seed_role(
            &db,
            &unique("find_deleted"),
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_disabled = find_by_id(&db, disabled.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

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
        let db = test_txn().await;
        let keyword = unique("page_keyword");
        let live = seed_role(&db, &format!("{keyword}_live"), 1, None).await;
        let disabled = seed_role(&db, &format!("{keyword}_disabled"), 0, None).await;
        let deleted = seed_role(
            &db,
            &format!("{keyword}_deleted"),
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let data_all = find_page(
            &db,
            &RoleFilter {
                keyword: Some(keyword.clone()),
                status: None,
                ..Default::default()
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
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(data_all.total, 2, "软删除角色不应进入分页");
        assert_eq!(data_all.items.len(), 2);
        assert_eq!(data_enabled.total, 1, "status 过滤应只保留启用角色");
        assert_eq!(data_enabled.items[0].id, live.id);
    }

    #[tokio::test]
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
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
            ACTOR_ID,
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
        let db = test_txn().await;

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
        mark_deleted.deleted_at = Set(Some(chrono::Local::now().naive_local()));
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

    /// 造一个操作人用户（直接 insert，不走 repo；其审计字段为 NULL 属预期）。
    async fn seed_actor(db: &impl ConnectionTrait) -> u64 {
        crate::entity::sys_user::ActiveModel {
            username: Set(unique("audit_actor")),
            password: Set("x".to_string()),
            nickname: Set("审计操作人".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn delete_actors(db: &impl ConnectionTrait, ids: &[u64]) {
        for id in ids {
            crate::entity::sys_user::Entity::delete_by_id(*id)
                .exec(db)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
    async fn create_role_with_links_stamps_actor_as_creator_and_updater() {
        let db = test_db().await;
        let actor_id = seed_actor(&db).await;

        let created = create_role_with_links(
            &db,
            sys_role::ActiveModel {
                role_name: Set(unique("audit_role")),
                role_key: Set(unique("audit_role_key")),
                status: Set(1),
                ..Default::default()
            },
            vec![],
            vec![],
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id);
        assert_eq!(created.updated_by, actor_id);

        cleanup(&db, &[created.id], &[], &[]).await;
        delete_actors(&db, &[actor_id]).await;
    }

    #[tokio::test]
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
    async fn update_role_with_links_refreshes_updated_by_and_keeps_created_by() {
        let db = test_db().await;
        let creator_id = seed_actor(&db).await;
        let updater_id = seed_actor(&db).await;

        let created = create_role_with_links(
            &db,
            sys_role::ActiveModel {
                role_name: Set(unique("audit_role")),
                role_key: Set(unique("audit_role_key")),
                status: Set(1),
                ..Default::default()
            },
            vec![],
            vec![],
            creator_id,
        )
        .await
        .unwrap();

        let updated = update_role_with_links(
            &db,
            sys_role::ActiveModel {
                id: Set(created.id),
                role_name: Set(unique("audit_role_renamed")),
                ..Default::default()
            },
            vec![],
            vec![],
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);

        cleanup(&db, &[created.id], &[], &[]).await;
        delete_actors(&db, &[creator_id, updater_id]).await;
    }

    #[tokio::test]
    // 例外：被测函数内部自带事务（sea-orm 1.1.20 真库无 savepoint，
    // 事务内嵌套 begin 会隐式提交），故用真实连接 + 手写清理。
    async fn update_role_refreshes_updated_by_for_status_change() {
        let db = test_db().await;
        let creator_id = seed_actor(&db).await;
        let updater_id = seed_actor(&db).await;

        let created = create_role_with_links(
            &db,
            sys_role::ActiveModel {
                role_name: Set(unique("audit_role")),
                role_key: Set(unique("audit_role_key")),
                status: Set(1),
                ..Default::default()
            },
            vec![],
            vec![],
            creator_id,
        )
        .await
        .unwrap();

        // 状态更新走通用 update_role（ActiveModel 由 Model 转换，字段全量 Set）
        let mut model: sys_role::ActiveModel = created.clone().into();
        model.status = Set(0);
        assert!(update_role(&db, model, updater_id).await.unwrap());

        let reloaded = find_by_id(&db, created.id).await.unwrap().unwrap();
        assert_eq!(reloaded.created_by, creator_id);
        assert_eq!(reloaded.updated_by, updater_id);

        cleanup(&db, &[created.id], &[], &[]).await;
        delete_actors(&db, &[creator_id, updater_id]).await;
    }

    /// 审计过滤不依赖 keyword：仅传 created_by（不传 keyword）也应生效。
    #[tokio::test]
    async fn find_page_filters_by_audit_columns_without_keyword() {
        let db = test_txn().await;
        let a = seed_role(&db, &unique("audit_nokw_a"), 1, None).await;
        let b = seed_role(&db, &unique("audit_nokw_b"), 1, None).await;

        // update_many 盖不同的审计人：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by) in [(a.id, 7_i64), (b.id, 8_i64)] {
            sys_role::Entity::update_many()
                .filter(sys_role::Column::Id.eq(row))
                .col_expr(sys_role::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_role::Column::UpdatedBy, Expr::value(by))
                .exec(&db)
                .await
                .unwrap();
        }

        let by_creator = find_page(
            &db,
            &RoleFilter {
                created_by: Some(7),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            by_creator.total, 1,
            "无 keyword 时 created_by=7 也应只命中 a"
        );
    }

    /// 审计字段过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique("audit_page");
        let a = seed_role(&db, &format!("{kw}a"), 1, None).await;
        let b = seed_role(&db, &format!("{kw}b"), 1, None).await;

        let base = chrono::Local::now().naive_local();
        // update_many 盖不同的审计人与时间：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_role::Entity::update_many()
                .filter(sys_role::Column::Id.eq(row))
                .col_expr(sys_role::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_role::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_role::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_role::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let all = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(all.total, 2, "前置：keyword 应命中两行");
        let by_creator = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                created_by: Some(7),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_creator.total, 1, "created_by=7 应只命中 a");
        let by_updater = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                updated_by: Some(8),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_updater.total, 1, "updated_by=8 应只命中 b");
        let ghost = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                created_by: Some(9_999_999_999),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(ghost.total, 0, "不存在的创建人应过滤为空");
        let created_after = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                created_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(created_after.total, 1, "begin=base-7s 应只剩 b（晚于阈值）");
        let created_before = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                created_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(created_before.total, 1, "end=base-7s 应只剩 a（早于阈值）");
        let updated_after = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                updated_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            updated_after.total, 1,
            "updated_at 范围与 created_at 同机制"
        );
        let updated_before = find_page(
            &db,
            &RoleFilter {
                keyword: Some(kw.clone()),
                updated_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(updated_before.total, 1);

        sys_role::Entity::delete_many()
            .filter(sys_role::Column::Id.is_in([a.id, b.id]))
            .exec(&db)
            .await
            .unwrap();
    }
}
