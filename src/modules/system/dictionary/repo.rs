//! 数据字典数据访问：类型表（`sys_dictionary`）与字典项表（`sys_dictionary_detail`）。
//! 集成测试直连 MySQL。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, QueryOrder};

use crate::entity::sys_dictionary::Model as Dictionary;
use crate::entity::sys_dictionary_detail::Model as Detail;
use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::modules::system::dictionary::dto::{DictionaryDetailFilter, DictionaryFilter};

// —— 字典类型 ——

/// 类型：按主键查有效记录（排除软删）。
pub async fn find_dictionary_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<Dictionary>> {
    let model = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Id.eq(id))
        .filter(sys_dictionary::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 类型：分页 + 动态过滤（keyword 对 name / type 模糊，status 精确，
/// created_by/updated_by/时间范围审计过滤），id 倒序。
pub async fn find_dictionary_page(
    db: &impl ConnectionTrait,
    filter: &DictionaryFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Dictionary>> {
    let mut cond = Condition::all();

    if let Some(v) = &filter.status {
        cond = cond.add(sys_dictionary::Column::Status.eq(*v));
    }
    if let Some(v) = &filter.keyword {
        let kw_cond = Condition::any()
            .add(sys_dictionary::Column::Name.like(format!("%{}%", v)))
            .add(sys_dictionary::Column::Type.like(format!("%{}%", v)));
        cond = cond.add(kw_cond);
    }

    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_dictionary::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_dictionary::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_dictionary::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_dictionary::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_dictionary::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_dictionary::Column::UpdatedAt.lte(v));
    }
    let select = sys_dictionary::Entity::find()
        .filter(cond)
        .filter(sys_dictionary::Column::DeletedAt.is_null())
        .order_by_desc(sys_dictionary::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 类型：按 type 编码查有效记录（排除软删），`get-by-type` 下拉校验用。
///
/// 注意与查重辅助 `find_dictionary_by_type_include_deleted` 的区别：
/// 本函数过滤软删，只服务「取当前可用类型」；查重语义必须走 include_deleted 版本。
pub async fn find_dictionary_by_type(
    db: &impl ConnectionTrait,
    r#type: &str,
) -> anyhow::Result<Option<Dictionary>> {
    let model = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(r#type))
        .filter(sys_dictionary::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    Ok(model)
}

/// 类型：查重辅助——type 唯一（含软删占位，不过滤 deleted_at）。
pub async fn find_dictionary_by_type_include_deleted(
    db: &impl ConnectionTrait,
    r#type: &str,
) -> anyhow::Result<Option<Dictionary>> {
    let model = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(r#type))
        .one(db)
        .await?;
    Ok(model)
}

/// 类型：创建。`actor_id` 为操作人，审计字段由 repo 统一盖章。
pub async fn create_dictionary(
    db: &impl ConnectionTrait,
    model: sys_dictionary::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Dictionary> {
    // 创建场景：创建人与更新人同源
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    let model = model.insert(db).await?;
    Ok(model)
}

/// 类型：更新（主键必须已设置）。审计字段由 repo 统一盖章：只刷新更新人。
pub async fn update_dictionary(
    db: &impl ConnectionTrait,
    model: sys_dictionary::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Dictionary> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    let model = model.update(db).await?;
    Ok(model)
}

/// 类型：软删单条，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_dictionary(
    db: &impl ConnectionTrait,
    id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    // 先查询是否存在，再软删
    let Some(model) = find_dictionary_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_dictionary::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.updated_by = Set(actor_id);
    model.update(db).await?;
    Ok(true)
}

// —— 字典项 ——

/// 字典项：按主键查有效记录（排除软删）。
pub async fn find_detail_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<Detail>> {
    let model = sys_dictionary_detail::Entity::find()
        .filter(sys_dictionary_detail::Column::Id.eq(id))
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 字典项：分页 + 动态过滤（dictionary_id 精确 + keyword 对 label/value 模糊
/// + status 精确 + created_by/updated_by/时间范围审计过滤），sort 升序、id 升序。
pub async fn find_detail_page(
    db: &impl ConnectionTrait,
    filter: &DictionaryDetailFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Detail>> {
    let mut cond = Condition::all();
    if let Some(v) = &filter.dictionary_id {
        cond = cond.add(sys_dictionary_detail::Column::DictionaryId.eq(*v));
    }
    if let Some(v) = &filter.keyword {
        let kw_cond = Condition::any()
            .add(sys_dictionary_detail::Column::Label.like(format!("%{}%", v)))
            .add(sys_dictionary_detail::Column::Value.like(format!("%{}%", v)));
        cond = cond.add(kw_cond);
    }
    if let Some(v) = &filter.status {
        cond = cond.add(sys_dictionary_detail::Column::Status.eq(*v));
    }
    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_dictionary_detail::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_dictionary_detail::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_dictionary_detail::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_dictionary_detail::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_dictionary_detail::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_dictionary_detail::Column::UpdatedAt.lte(v));
    }
    let select = sys_dictionary_detail::Entity::find()
        .filter(cond)
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .order_by_asc(sys_dictionary_detail::Column::Sort)
        .order_by_asc(sys_dictionary_detail::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 字典项：取某类型下全部启用且未软删的项，sort 升序、id 升序（`get-by-type` 用）。
pub async fn find_enabled_details(
    db: &impl ConnectionTrait,
    dictionary_id: u64,
) -> anyhow::Result<Vec<Detail>> {
    let details = sys_dictionary_detail::Entity::find()
        .filter(sys_dictionary_detail::Column::DictionaryId.eq(dictionary_id))
        .filter(sys_dictionary_detail::Column::Status.eq(1))
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .order_by_asc(sys_dictionary_detail::Column::Sort)
        .order_by_asc(sys_dictionary_detail::Column::Id)
        .all(db)
        .await?;
    Ok(details)
}

/// 字典项：查重辅助——同类型同 value 的活记录（软删的不算）。
pub async fn find_alive_detail_by_value(
    db: &impl ConnectionTrait,
    dictionary_id: u64,
    value: &str,
) -> anyhow::Result<Option<Detail>> {
    let model = sys_dictionary_detail::Entity::find()
        .filter(sys_dictionary_detail::Column::DictionaryId.eq(dictionary_id))
        .filter(sys_dictionary_detail::Column::Value.eq(value))
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 字典项：级联软删某类型下全部活记录，返回受影响行数。
pub async fn soft_delete_details_by_dictionary_id(
    db: &impl ConnectionTrait,
    dictionary_id: u64,
) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let result = sys_dictionary_detail::Entity::update_many()
        .col_expr(
            sys_dictionary_detail::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(Some(now)),
        )
        .filter(sys_dictionary_detail::Column::DictionaryId.eq(dictionary_id))
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// 字典项：创建。`actor_id` 为操作人，审计字段由 repo 统一盖章。
pub async fn create_detail(
    db: &impl ConnectionTrait,
    model: sys_dictionary_detail::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Detail> {
    // 创建场景：创建人与更新人同源
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    Ok(model.insert(db).await?)
}

/// 字典项：更新（主键必须已设置）。审计字段由 repo 统一盖章：只刷新更新人。
pub async fn update_detail(
    db: &impl ConnectionTrait,
    model: sys_dictionary_detail::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Detail> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    Ok(model.update(db).await?)
}

/// 字典项：软删单条，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_detail(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_detail_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_dictionary_detail::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
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

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    fn now() -> chrono::NaiveDateTime {
        chrono::Local::now().naive_local()
    }

    async fn seed_dictionary(
        db: &impl ConnectionTrait,
        name: &str,
        r#type: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> Dictionary {
        sys_dictionary::ActiveModel {
            name: Set(name.to_string()),
            r#type: Set(r#type.to_string()),
            status: Set(status),
            remark: Set(String::new()),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_detail(
        db: &impl ConnectionTrait,
        dictionary_id: u64,
        value: &str,
        label: &str,
        sort: i32,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> Detail {
        sys_dictionary_detail::ActiveModel {
            dictionary_id: Set(dictionary_id),
            label: Set(label.to_string()),
            value: Set(value.to_string()),
            extend: Set(String::new()),
            sort: Set(sort),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 测后清理：先物理删字典项，再删字典类型。
    async fn cleanup(db: &impl ConnectionTrait, dictionary_ids: &[u64]) {
        sys_dictionary_detail::Entity::delete_many()
            .filter(
                sys_dictionary_detail::Column::DictionaryId.is_in(dictionary_ids.iter().copied()),
            )
            .exec(db)
            .await
            .unwrap();
        sys_dictionary::Entity::delete_many()
            .filter(sys_dictionary::Column::Id.is_in(dictionary_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn find_dictionary_page_filters_by_keyword_and_status_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("kw");
        // 命中 name，启用
        let by_name = seed_dictionary(&db, &format!("名称{kw}"), &unique("t"), 1, None).await;
        // 命中 type，启用
        let by_type = seed_dictionary(&db, &unique("nm"), &format!("t{kw}"), 1, None).await;
        // 命中 name 但禁用：只有 status 过滤能排除它
        let disabled = seed_dictionary(&db, &format!("禁用{kw}"), &unique("t2"), 0, None).await;
        // 与 kw 无关，启用：只有 keyword 过滤能排除它
        let unrelated = seed_dictionary(&db, &unique("nm2"), &unique("t3"), 1, None).await;
        // 命中 name 但已软删：keyword / status 都必须排除它
        let deleted =
            seed_dictionary(&db, &format!("软删{kw}"), &unique("tdel"), 1, Some(now())).await;

        // 仅 keyword：命中 name / type 的启用与禁用记录都在，软删与无关记录排除。
        // 注意：查询必须用 kw 圈定范围，避免依赖「全表只有本测试数据」这一不成立前提。
        let all = find_dictionary_page(
            &db,
            &DictionaryFilter {
                keyword: Some(kw.clone()),
                status: None,
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        // keyword + status=1：禁用与软删记录排除，只剩命中 name / type 的启用记录
        let enabled = find_dictionary_page(
            &db,
            &DictionaryFilter {
                keyword: Some(kw.clone()),
                status: Some(1),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(
            &db,
            &[
                by_name.id,
                by_type.id,
                disabled.id,
                unrelated.id,
                deleted.id,
            ],
        )
        .await;

        let mut all_ids: Vec<u64> = all.items.iter().map(|m| m.id).collect();
        all_ids.sort_unstable();
        let mut expected_all = vec![by_name.id, by_type.id, disabled.id];
        expected_all.sort_unstable();
        assert_eq!(all.total, 3, "keyword 应同时命中 name 与 type");
        assert_eq!(all_ids, expected_all, "软删与无关记录不应出现");
        assert!(all.items[0].id > all.items[1].id, "类型列表应按 id 倒序");

        let mut enabled_ids: Vec<u64> = enabled.items.iter().map(|m| m.id).collect();
        enabled_ids.sort_unstable();
        let mut expected_enabled = vec![by_name.id, by_type.id];
        expected_enabled.sort_unstable();
        assert_eq!(enabled.total, 2, "status=1 应排除禁用记录与软删记录");
        assert_eq!(enabled_ids, expected_enabled);
    }

    #[tokio::test]
    async fn find_dictionary_by_type_include_deleted_finds_soft_deleted() {
        let db = test_txn().await;
        let live = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let deleted = seed_dictionary(&db, &unique("nm"), &unique("tdel"), 1, Some(now())).await;

        let found_live = find_dictionary_by_type_include_deleted(&db, &live.r#type)
            .await
            .unwrap();
        let found_deleted = find_dictionary_by_type_include_deleted(&db, &deleted.r#type)
            .await
            .unwrap();

        assert_eq!(found_live.map(|m| m.id), Some(live.id));
        assert_eq!(
            found_deleted.map(|m| m.id),
            Some(deleted.id),
            "type 唯一键含软删占位，软删记录必须能查到"
        );
    }

    #[tokio::test]
    async fn find_detail_page_filters_by_dictionary_and_keyword_excludes_deleted() {
        let db = test_txn().await;
        let d1 = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let d2 = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let kw = unique("kw");
        let hit_label =
            seed_detail(&db, d1.id, &unique("v"), &format!("显示{kw}"), 0, 1, None).await;
        let hit_value = seed_detail(&db, d1.id, &format!("v{kw}"), &unique("lb"), 0, 1, None).await;
        let in_other = seed_detail(&db, d2.id, &format!("v{kw}"), &unique("lb"), 0, 1, None).await;
        // 软删记录用于构造「应被排除」的场景，断言只校验活记录，无需引用其 id
        let _deleted = seed_detail(
            &db,
            d1.id,
            &format!("v{kw}"),
            &unique("lb"),
            0,
            1,
            Some(now()),
        )
        .await;

        let page = find_detail_page(
            &db,
            &DictionaryDetailFilter {
                dictionary_id: Some(d1.id),
                keyword: Some(kw.clone()),
                status: None,
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let other_page = find_detail_page(
            &db,
            &DictionaryDetailFilter {
                dictionary_id: Some(d2.id),
                keyword: None,
                status: None,
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(page.total, 2, "keyword 应命中 label 与 value 且排除软删");
        let mut ids: Vec<u64> = page.items.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        let mut expected = vec![hit_label.id, hit_value.id];
        expected.sort_unstable();
        assert_eq!(ids, expected, "其它类型与软删记录不应出现");
        assert_eq!(other_page.total, 1, "dictionary_id 过滤应生效");
        assert_eq!(other_page.items[0].id, in_other.id);
    }

    #[tokio::test]
    async fn find_enabled_details_only_alive_and_enabled_sorted_by_sort() {
        let db = test_txn().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let second = seed_detail(&db, d.id, &unique("v"), "lb", 2, 1, None).await;
        let first = seed_detail(&db, d.id, &unique("v"), "lb", 1, 1, None).await;
        let _disabled = seed_detail(&db, d.id, &unique("v"), "lb", 0, 0, None).await;
        let _deleted = seed_detail(&db, d.id, &unique("v"), "lb", 0, 1, Some(now())).await;

        let items = find_enabled_details(&db, d.id).await.unwrap();

        let ids: Vec<u64> = items.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![first.id, second.id], "只返回启用项且按 sort 升序");
    }

    #[tokio::test]
    async fn soft_delete_details_by_dictionary_id_marks_all_alive_rows() {
        let db = test_txn().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let _a = seed_detail(&db, d.id, &unique("v"), "lb", 1, 1, None).await;
        let _b = seed_detail(&db, d.id, &unique("v"), "lb", 2, 1, None).await;
        let _c = seed_detail(&db, d.id, &unique("v"), "lb", 3, 1, Some(now())).await;

        let affected = soft_delete_details_by_dictionary_id(&db, d.id)
            .await
            .unwrap();
        let left = find_enabled_details(&db, d.id).await.unwrap();

        assert_eq!(affected, 2, "只统计活记录，已软删的不重复处理");
        assert!(left.is_empty(), "级联软删后该类型下不应再有启用项");
    }

    #[tokio::test]
    async fn find_alive_detail_by_value_ignores_soft_deleted() {
        let db = test_txn().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let _old = seed_detail(&db, d.id, "dup_value", "lb", 0, 1, Some(now())).await;

        let found_deleted = find_alive_detail_by_value(&db, d.id, "dup_value")
            .await
            .unwrap();
        let alive = seed_detail(&db, d.id, "dup_value", "lb", 0, 1, None).await;
        let found_alive = find_alive_detail_by_value(&db, d.id, "dup_value")
            .await
            .unwrap();

        assert!(
            found_deleted.is_none(),
            "软删占位不算活记录：同 value 应允许重建"
        );
        assert_eq!(found_alive.map(|m| m.id), Some(alive.id));
    }

    /// 造一个操作人用户（直接 insert，不走 repo；其审计字段为 0（种子/系统写入口径）属预期）。
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
        crate::entity::sys_user::Entity::delete_many()
            .filter(crate::entity::sys_user::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_dictionary_stamps_actor_as_creator_and_updater() {
        let db = test_txn().await;
        let actor_id = seed_actor(&db).await;

        let created = create_dictionary(
            &db,
            sys_dictionary::ActiveModel {
                name: Set(unique("audit_dict")),
                r#type: Set(unique("audit_dict_type")),
                status: Set(1),
                ..Default::default()
            },
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id);
        assert_eq!(created.updated_by, actor_id);

        delete_actors(&db, &[actor_id]).await;
    }

    #[tokio::test]
    async fn update_dictionary_refreshes_updated_by_and_keeps_created_by() {
        let db = test_txn().await;
        let creator_id = seed_actor(&db).await;
        let updater_id = seed_actor(&db).await;

        let created = create_dictionary(
            &db,
            sys_dictionary::ActiveModel {
                name: Set(unique("audit_dict")),
                r#type: Set(unique("audit_dict_type")),
                status: Set(1),
                ..Default::default()
            },
            creator_id,
        )
        .await
        .unwrap();

        let updated = update_dictionary(
            &db,
            sys_dictionary::ActiveModel {
                id: Set(created.id),
                name: Set(unique("audit_dict_renamed")),
                ..Default::default()
            },
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);

        delete_actors(&db, &[creator_id, updater_id]).await;
    }

    #[tokio::test]
    async fn create_detail_stamps_actor_as_creator_and_updater() {
        let db = test_txn().await;
        let actor_id = seed_actor(&db).await;
        let dict = seed_dictionary(&db, "审计字典", &unique("audit_type"), 1, None).await;

        let created = create_detail(
            &db,
            sys_dictionary_detail::ActiveModel {
                dictionary_id: Set(dict.id),
                label: Set(unique("audit_label")),
                value: Set(unique("audit_value")),
                status: Set(1),
                ..Default::default()
            },
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id);
        assert_eq!(created.updated_by, actor_id);

        delete_actors(&db, &[actor_id]).await;
    }

    #[tokio::test]
    async fn update_detail_refreshes_updated_by_and_keeps_created_by() {
        let db = test_txn().await;
        let creator_id = seed_actor(&db).await;
        let updater_id = seed_actor(&db).await;
        let dict = seed_dictionary(&db, "审计字典", &unique("audit_type"), 1, None).await;

        let created = create_detail(
            &db,
            sys_dictionary_detail::ActiveModel {
                dictionary_id: Set(dict.id),
                label: Set(unique("audit_label")),
                value: Set(unique("audit_value")),
                status: Set(1),
                ..Default::default()
            },
            creator_id,
        )
        .await
        .unwrap();

        let updated = update_detail(
            &db,
            sys_dictionary_detail::ActiveModel {
                id: Set(created.id),
                label: Set(unique("audit_label_renamed")),
                ..Default::default()
            },
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);

        delete_actors(&db, &[creator_id, updater_id]).await;
    }

    /// 审计字段过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique("audit_page");
        let a = seed_dictionary(&db, &format!("{kw}a"), &unique("audit_type"), 1, None).await;
        let b = seed_dictionary(&db, &format!("{kw}b"), &unique("audit_type"), 1, None).await;

        let base = chrono::Local::now().naive_local();
        // update_many 盖不同的审计人与时间：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_dictionary::Entity::update_many()
                .filter(sys_dictionary::Column::Id.eq(row))
                .col_expr(sys_dictionary::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_dictionary::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_dictionary::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_dictionary::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let all = find_dictionary_page(
            &db,
            &DictionaryFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(all.total, 2, "前置：keyword 应命中两行");
        let by_creator = find_dictionary_page(
            &db,
            &DictionaryFilter {
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
        let by_updater = find_dictionary_page(
            &db,
            &DictionaryFilter {
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
        let ghost = find_dictionary_page(
            &db,
            &DictionaryFilter {
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
        let created_after = find_dictionary_page(
            &db,
            &DictionaryFilter {
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
        let created_before = find_dictionary_page(
            &db,
            &DictionaryFilter {
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
        let updated_after = find_dictionary_page(
            &db,
            &DictionaryFilter {
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
        let updated_before = find_dictionary_page(
            &db,
            &DictionaryFilter {
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

        sys_dictionary::Entity::delete_many()
            .filter(sys_dictionary::Column::Id.is_in([a.id, b.id]))
            .exec(&db)
            .await
            .unwrap();
    }

    /// 字典项审计过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_detail_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique("audit_detail");
        let dict =
            seed_dictionary(&db, &unique("audit_dict"), &unique("audit_dtype"), 1, None).await;
        let a = seed_detail(
            &db,
            dict.id,
            &format!("{kw}a"),
            &format!("项{kw}a"),
            0,
            1,
            None,
        )
        .await;
        let b = seed_detail(
            &db,
            dict.id,
            &format!("{kw}b"),
            &format!("项{kw}b"),
            0,
            1,
            None,
        )
        .await;
        let base = chrono::Local::now().naive_local();
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_dictionary_detail::Entity::update_many()
                .filter(sys_dictionary_detail::Column::Id.eq(row))
                .col_expr(sys_dictionary_detail::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_dictionary_detail::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_dictionary_detail::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_dictionary_detail::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }
        let all = find_detail_page(
            &db,
            &DictionaryDetailFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(all.total, 2, "前置：keyword 应命中两行");
        let by_creator = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        let by_updater = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        let ghost = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        let created_after = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        let created_before = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        let updated_after = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        let updated_before = find_detail_page(
            &db,
            &DictionaryDetailFilter {
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
        sys_dictionary_detail::Entity::delete_many()
            .filter(sys_dictionary_detail::Column::Id.is_in([a.id, b.id]))
            .exec(&db)
            .await
            .unwrap();
        sys_dictionary::Entity::delete_many()
            .filter(sys_dictionary::Column::Id.eq(dict.id))
            .exec(&db)
            .await
            .unwrap();
    }
}
