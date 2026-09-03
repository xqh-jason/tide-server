//! 数据字典数据访问：类型表（`sys_dictionary`）与字典项表（`sys_dictionary_detail`）。
//!
//! 函数体按实现计划任务 5 补全；下方 `#[cfg(test)]` 集成测试直连 MySQL，由 AI 编写。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, QueryOrder};

use crate::entity::sys_dictionary::Model as Dictionary;
use crate::entity::sys_dictionary_detail::Model as Detail;
use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::modules::dictionary::dto::{DictionaryDetailFilter, DictionaryFilter};

// —— 字典类型 ——

/// 类型：按主键查有效记录（排除软删）。
pub async fn find_dictionary_by_id(
    db: &DatabaseConnection,
    id: u64,
) -> anyhow::Result<Option<Dictionary>> {
    let model = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Id.eq(id))
        .filter(sys_dictionary::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 类型：分页 + 动态过滤（keyword 对 name / type 模糊，status 精确），id 倒序。
pub async fn find_dictionary_page(
    db: &DatabaseConnection,
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
    db: &DatabaseConnection,
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
    db: &DatabaseConnection,
    r#type: &str,
) -> anyhow::Result<Option<Dictionary>> {
    let model = sys_dictionary::Entity::find()
        .filter(sys_dictionary::Column::Type.eq(r#type))
        .one(db)
        .await?;
    Ok(model)
}

/// 类型：创建。
pub async fn create_dictionary(
    db: &DatabaseConnection,
    model: sys_dictionary::ActiveModel,
) -> anyhow::Result<Dictionary> {
    let model = model.insert(db).await?;
    Ok(model)
}

/// 类型：更新（主键必须已设置）。
pub async fn update_dictionary(
    db: &DatabaseConnection,
    model: sys_dictionary::ActiveModel,
) -> anyhow::Result<Dictionary> {
    let model = model.update(db).await?;
    Ok(model)
}

/// 类型：软删单条，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_dictionary(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    // 先查询是否存在，再软删
    let Some(model) = find_dictionary_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_dictionary::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

// —— 字典项 ——

/// 字典项：按主键查有效记录（排除软删）。
pub async fn find_detail_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Detail>> {
    let model = sys_dictionary_detail::Entity::find()
        .filter(sys_dictionary_detail::Column::Id.eq(id))
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 字典项：分页 + 动态过滤（dictionary_id 精确 + keyword 对 label/value 模糊
/// + status 精确），sort 升序、id 升序。
pub async fn find_detail_page(
    db: &DatabaseConnection,
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
    let select = sys_dictionary_detail::Entity::find()
        .filter(cond)
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .order_by_asc(sys_dictionary_detail::Column::Sort)
        .order_by_asc(sys_dictionary_detail::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 字典项：取某类型下全部启用且未软删的项，sort 升序、id 升序（`get-by-type` 用）。
pub async fn find_enabled_details(
    db: &DatabaseConnection,
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

/// 字典项：查重辅助——同类型同 value 的活记录（软删的不算，见规格 §3.3）。
pub async fn find_alive_detail_by_value(
    db: &DatabaseConnection,
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
    db: &DatabaseConnection,
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

/// 字典项：创建。
pub async fn create_detail(
    db: &DatabaseConnection,
    model: sys_dictionary_detail::ActiveModel,
) -> anyhow::Result<Detail> {
    Ok(model.insert(db).await?)
}

/// 字典项：更新（主键必须已设置）。
pub async fn update_detail(
    db: &DatabaseConnection,
    model: sys_dictionary_detail::ActiveModel,
) -> anyhow::Result<Detail> {
    Ok(model.update(db).await?)
}

/// 字典项：软删单条，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_detail(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
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

    fn now() -> chrono::NaiveDateTime {
        chrono::Utc::now().naive_utc()
    }

    async fn seed_dictionary(
        db: &DatabaseConnection,
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
        db: &DatabaseConnection,
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
    async fn cleanup(db: &DatabaseConnection, dictionary_ids: &[u64]) {
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
        let db = test_db().await;
        let kw = unique("kw");
        // 命中 name，启用
        let by_name = seed_dictionary(&db, &format!("名称{kw}"), &unique("t"), 1, None).await;
        // 命中 type，启用
        let by_type = seed_dictionary(&db, &unique("nm"), &format!("t{kw}"), 1, None).await;
        // 命中 name 但停用：只有 status 过滤能排除它
        let disabled = seed_dictionary(&db, &format!("禁用{kw}"), &unique("t2"), 0, None).await;
        // 与 kw 无关，启用：只有 keyword 过滤能排除它
        let unrelated = seed_dictionary(&db, &unique("nm2"), &unique("t3"), 1, None).await;
        // 命中 name 但已软删：keyword / status 都必须排除它
        let deleted =
            seed_dictionary(&db, &format!("软删{kw}"), &unique("tdel"), 1, Some(now())).await;

        // 仅 keyword：命中 name / type 的启用与停用记录都在，软删与无关记录排除。
        // 注意：查询必须用 kw 圈定范围，避免依赖「全表只有本测试数据」这一不成立前提。
        let all = find_dictionary_page(
            &db,
            &DictionaryFilter {
                keyword: Some(kw.clone()),
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        // keyword + status=1：停用与软删记录排除，只剩命中 name / type 的启用记录
        let enabled = find_dictionary_page(
            &db,
            &DictionaryFilter {
                keyword: Some(kw.clone()),
                status: Some(1),
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
        assert_eq!(enabled.total, 2, "status=1 应排除停用记录与软删记录");
        assert_eq!(enabled_ids, expected_enabled);
    }

    #[tokio::test]
    async fn find_dictionary_by_type_include_deleted_finds_soft_deleted() {
        let db = test_db().await;
        let live = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let deleted = seed_dictionary(&db, &unique("nm"), &unique("tdel"), 1, Some(now())).await;

        let found_live = find_dictionary_by_type_include_deleted(&db, &live.r#type)
            .await
            .unwrap();
        let found_deleted = find_dictionary_by_type_include_deleted(&db, &deleted.r#type)
            .await
            .unwrap();

        cleanup(&db, &[live.id, deleted.id]).await;

        assert_eq!(found_live.map(|m| m.id), Some(live.id));
        assert_eq!(
            found_deleted.map(|m| m.id),
            Some(deleted.id),
            "type 唯一键含软删占位，软删记录必须能查到"
        );
    }

    #[tokio::test]
    async fn find_detail_page_filters_by_dictionary_and_keyword_excludes_deleted() {
        let db = test_db().await;
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
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[d1.id, d2.id]).await;

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
        let db = test_db().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let second = seed_detail(&db, d.id, &unique("v"), "lb", 2, 1, None).await;
        let first = seed_detail(&db, d.id, &unique("v"), "lb", 1, 1, None).await;
        let _disabled = seed_detail(&db, d.id, &unique("v"), "lb", 0, 0, None).await;
        let _deleted = seed_detail(&db, d.id, &unique("v"), "lb", 0, 1, Some(now())).await;

        let items = find_enabled_details(&db, d.id).await.unwrap();
        cleanup(&db, &[d.id]).await;

        let ids: Vec<u64> = items.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![first.id, second.id], "只返回启用项且按 sort 升序");
    }

    #[tokio::test]
    async fn soft_delete_details_by_dictionary_id_marks_all_alive_rows() {
        let db = test_db().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let _a = seed_detail(&db, d.id, &unique("v"), "lb", 1, 1, None).await;
        let _b = seed_detail(&db, d.id, &unique("v"), "lb", 2, 1, None).await;
        let _c = seed_detail(&db, d.id, &unique("v"), "lb", 3, 1, Some(now())).await;

        let affected = soft_delete_details_by_dictionary_id(&db, d.id)
            .await
            .unwrap();
        let left = find_enabled_details(&db, d.id).await.unwrap();
        cleanup(&db, &[d.id]).await;

        assert_eq!(affected, 2, "只统计活记录，已软删的不重复处理");
        assert!(left.is_empty(), "级联软删后该类型下不应再有启用项");
    }

    #[tokio::test]
    async fn find_alive_detail_by_value_ignores_soft_deleted() {
        let db = test_db().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let _old = seed_detail(&db, d.id, "dup_value", "lb", 0, 1, Some(now())).await;

        let found_deleted = find_alive_detail_by_value(&db, d.id, "dup_value")
            .await
            .unwrap();
        let alive = seed_detail(&db, d.id, "dup_value", "lb", 0, 1, None).await;
        let found_alive = find_alive_detail_by_value(&db, d.id, "dup_value")
            .await
            .unwrap();

        cleanup(&db, &[d.id]).await;

        assert!(
            found_deleted.is_none(),
            "软删占位不算活记录：同 value 应允许重建"
        );
        assert_eq!(found_alive.map(|m| m.id), Some(alive.id));
    }
}
