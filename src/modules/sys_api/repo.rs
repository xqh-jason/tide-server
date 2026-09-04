//! API 权限点数据访问。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, TransactionTrait};

use crate::entity::{sys_api, sys_api::Model, sys_role_api};
use crate::modules::sys_api::dto::ApiFilter;

/// 查询单个有效 API（排除软删除）。
pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    let api = sys_api::Entity::find()
        .filter(sys_api::Column::Id.eq(id))
        .filter(sys_api::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(api)
}

/// 分页 + 动态过滤查询（keyword 匹配 path/description/api_group，status/method 精确）。
pub async fn find_page(
    db: &DatabaseConnection,
    filter: &ApiFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(status) = filter.status {
        cond = cond.add(sys_api::Column::Status.eq(status));
    }
    if let Some(method) = &filter.method {
        cond = cond.add(sys_api::Column::Method.eq(method));
    }
    if let Some(kw) = &filter.keyword {
        let kw_cond = Condition::any()
            .add(sys_api::Column::Path.like(format!("%{kw}%")))
            .add(sys_api::Column::Description.like(format!("%{kw}%")))
            .add(sys_api::Column::ApiGroup.like(format!("%{kw}%")));
        cond = cond.add(kw_cond);
    }

    let select = sys_api::Entity::find()
        .filter(cond)
        .filter(sys_api::Column::DeletedAt.is_null());
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 事务写入 API 并维护角色授权关联（关系表硬删除，只插不判软删）。
pub async fn create_api_with_links(
    db: &DatabaseConnection,
    api: sys_api::ActiveModel,
    role_ids: Vec<u64>,
) -> anyhow::Result<Model> {
    let txn = db.begin().await?;
    let api = api.insert(&txn).await?;
    if !role_ids.is_empty() {
        let role_api_ids = role_ids
            .into_iter()
            .map(|role_id| sys_role_api::ActiveModel {
                role_id: Set(role_id),
                api_id: Set(api.id),
            })
            .collect::<Vec<_>>();
        sys_role_api::Entity::insert_many(role_api_ids)
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(api)
}

/// 事务更新 API 并重建角色授权关联（先删旧关联，再插新关联）。
pub async fn update_api_with_links(
    db: &DatabaseConnection,
    api: sys_api::ActiveModel,
    role_ids: Vec<u64>,
) -> anyhow::Result<Model> {
    let txn = db.begin().await?;
    let api = api.update(&txn).await?;
    sys_role_api::Entity::delete_many()
        .filter(sys_role_api::Column::ApiId.eq(api.id))
        .exec(&txn)
        .await?;
    if !role_ids.is_empty() {
        let role_api_ids = role_ids
            .into_iter()
            .map(|role_id| sys_role_api::ActiveModel {
                role_id: Set(role_id),
                api_id: Set(api.id),
            })
            .collect::<Vec<_>>();
        sys_role_api::Entity::insert_many(role_api_ids)
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(api)
}

/// 软删除 API：同一事务内物理清空 `sys_role_api` 关联，再软删主表。
pub async fn soft_delete_api(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    let Some(api) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let txn = db.begin().await?;
    sys_role_api::Entity::delete_many()
        .filter(sys_role_api::Column::ApiId.eq(id))
        .exec(&txn)
        .await?;
    let mut api: sys_api::ActiveModel = api.into();
    api.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    api.update(&txn).await?;
    txn.commit().await?;
    Ok(true)
}

/// 查重辅助：path + method 组合（含软删记录占位）。
pub async fn find_by_path_method_include_deleted(
    db: &DatabaseConnection,
    path: &str,
    method: &str,
) -> anyhow::Result<Option<Model>> {
    let api = sys_api::Entity::find()
        .filter(sys_api::Column::Path.eq(path))
        .filter(sys_api::Column::Method.eq(method))
        .one(db)
        .await?;
    Ok(api)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_role, sys_role_api};
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

    async fn seed_api(
        db: &DatabaseConnection,
        path: &str,
        method: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_api::Model {
        sys_api::ActiveModel {
            path: Set(path.to_string()),
            method: Set(method.to_string()),
            description: Set(unique("api_desc")),
            api_group: Set("repo_test".to_string()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(db: &DatabaseConnection) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("api_role")),
            role_key: Set(unique("api_role_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 清理顺序：先删关联表，再删主表。
    async fn cleanup(db: &DatabaseConnection, api_ids: &[u64], role_ids: &[u64]) {
        for api_id in api_ids {
            sys_role_api::Entity::delete_many()
                .filter(sys_role_api::Column::ApiId.eq(*api_id))
                .exec(db)
                .await
                .unwrap();
        }
        sys_api::Entity::delete_many()
            .filter(sys_api::Column::Id.is_in(api_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_many()
            .filter(sys_role::Column::Id.is_in(role_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    /// 创建 API：事务写入主表 + 角色授权关联。
    #[tokio::test]
    async fn create_api_with_links_persists_api_and_links() {
        let db = test_db().await;
        let role_a = seed_role(&db).await;
        let role_b = seed_role(&db).await;
        let path = format!("/api/v1/{}/list", unique("create"));

        let created = create_api_with_links(
            &db,
            sys_api::ActiveModel {
                path: Set(path.clone()),
                method: Set("POST".to_string()),
                description: Set("创建测试".to_string()),
                api_group: Set("repo_test".to_string()),
                status: Set(1),
                ..Default::default()
            },
            vec![role_a.id, role_b.id],
        )
        .await
        .unwrap();

        let links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::ApiId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(&db, &[created.id], &[role_a.id, role_b.id]).await;

        assert_eq!(created.path, path);
        assert_eq!(links.len(), 2, "两个角色授权应全部落库");
    }

    /// 空角色列表也能创建 API（不触发无效 SQL）。
    #[tokio::test]
    async fn create_api_with_links_supports_empty_role_ids() {
        let db = test_db().await;
        let created = create_api_with_links(
            &db,
            sys_api::ActiveModel {
                path: Set(format!("/api/v1/{}/empty", unique("create"))),
                method: Set("POST".to_string()),
                description: Set(String::new()),
                api_group: Set("repo_test".to_string()),
                status: Set(1),
                ..Default::default()
            },
            vec![],
        )
        .await
        .unwrap();

        cleanup(&db, &[created.id], &[]).await;

        assert_eq!(created.status, 1);
    }

    /// find_by_id 排除软删除。
    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted_api() {
        let db = test_db().await;
        let live = seed_api(&db, &format!("/live_{}", unique("find")), "POST", 1, None).await;
        let deleted = seed_api(
            &db,
            &format!("/deleted_{}", unique("find")),
            "POST",
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        cleanup(&db, &[live.id, deleted.id], &[]).await;

        assert_eq!(found_live.as_ref().map(|a| a.id), Some(live.id));
        assert!(found_deleted.is_none(), "软删除 API 不应被 find_by_id 查到");
    }

    /// 分页：keyword / status / method 过滤，排除软删。
    #[tokio::test]
    async fn find_page_filters_by_keyword_status_method_excludes_deleted() {
        let db = test_db().await;
        let keyword = unique("page_keyword");
        let live = seed_api(&db, &format!("/{keyword}_live"), "POST", 1, None).await;
        let get_api = seed_api(&db, &format!("/{keyword}_get"), "GET", 1, None).await;
        let disabled = seed_api(&db, &format!("/{keyword}_disabled"), "POST", 0, None).await;
        let deleted = seed_api(
            &db,
            &format!("/{keyword}_deleted"),
            "POST",
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let data_all = find_page(
            &db,
            &ApiFilter {
                keyword: Some(keyword.clone()),
                status: None,
                method: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let data_post = find_page(
            &db,
            &ApiFilter {
                keyword: Some(keyword.clone()),
                status: None,
                method: Some("POST".to_string()),
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[live.id, get_api.id, disabled.id, deleted.id], &[]).await;

        assert_eq!(data_all.total, 3, "软删除 API 不应进入分页");
        assert_eq!(data_post.total, 2, "method 过滤应只保留 POST 且启用");
    }

    /// 更新 API：事务内重建角色授权关联（旧关联清空、新关联落库）。
    #[tokio::test]
    async fn update_api_with_links_rebuilds_links_in_transaction() {
        let db = test_db().await;
        let role_old = seed_role(&db).await;
        let role_new = seed_role(&db).await;
        let created = create_api_with_links(
            &db,
            sys_api::ActiveModel {
                path: Set(format!("/api/v1/{}/update", unique("update"))),
                method: Set("POST".to_string()),
                description: Set(String::new()),
                api_group: Set("repo_test".to_string()),
                status: Set(1),
                ..Default::default()
            },
            vec![role_old.id],
        )
        .await
        .unwrap();

        let mut model: sys_api::ActiveModel = created.clone().into();
        model.description = Set("更新后描述".to_string());
        let updated = update_api_with_links(&db, model, vec![role_new.id])
            .await
            .unwrap();

        let links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::ApiId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(&db, &[created.id], &[role_old.id, role_new.id]).await;

        assert_eq!(updated.description, "更新后描述");
        assert_eq!(links.len(), 1, "旧关联应被清空，只剩新关联");
        assert_eq!(links[0].role_id, role_new.id);
    }

    /// 软删除：物理清空授权关联，主表不可再被 find_by_id 查到。
    #[tokio::test]
    async fn soft_delete_api_removes_links_and_excludes_api() {
        let db = test_db().await;
        let role = seed_role(&db).await;
        let created = create_api_with_links(
            &db,
            sys_api::ActiveModel {
                path: Set(format!("/api/v1/{}/delete", unique("delete"))),
                method: Set("POST".to_string()),
                description: Set(String::new()),
                api_group: Set("repo_test".to_string()),
                status: Set(1),
                ..Default::default()
            },
            vec![role.id],
        )
        .await
        .unwrap();

        let deleted = soft_delete_api(&db, created.id).await.unwrap();
        let after = find_by_id(&db, created.id).await.unwrap();
        let links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::ApiId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(&db, &[created.id], &[role.id]).await;

        assert!(deleted);
        assert!(after.is_none(), "软删后 find_by_id 不应查到 API");
        assert!(links.is_empty(), "软删应物理清空角色授权关联");
    }

    /// 查重辅助：path + method 组合（含软删记录占位）。
    #[tokio::test]
    async fn find_by_path_method_include_deleted_finds_soft_deleted_api() {
        let db = test_db().await;
        let path = format!("/api/v1/{}/dup", unique("dup"));
        let deleted = seed_api(&db, &path, "POST", 1, Some(chrono::Local::now().naive_local())).await;

        let found = find_by_path_method_include_deleted(&db, &path, "POST")
            .await
            .unwrap();

        cleanup(&db, &[deleted.id], &[]).await;

        assert_eq!(found.as_ref().map(|a| a.id), Some(deleted.id));
    }
}
