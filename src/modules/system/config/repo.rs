//! 系统配置数据访问：键值参数（sys_config）与网站设置（sys_site_config）。
//!
//! 审计字段由 repo 统一盖章；查重含软删占位与 dictionary 的 type 同语义。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, QueryOrder};

use crate::entity::{sys_config, sys_site_config};
use crate::modules::system::config::dto::ConfigFilter;

// —— 键值参数 ——

/// 参数：按主键查有效记录（排除软删）。
pub async fn find_config_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<sys_config::Model>> {
    let model = sys_config::Entity::find()
        .filter(sys_config::Column::Id.eq(id))
        .filter(sys_config::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 参数：分页 + 动态过滤（keyword 对 name / key 模糊，
/// created_by/updated_by/时间范围审计过滤），排除软删，created_at 倒序。
pub async fn find_config_page(
    db: &impl ConnectionTrait,
    filter: &ConfigFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<sys_config::Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = filter.keyword.as_deref() {
        cond = cond.add(
            Condition::any()
                .add(sys_config::Column::ConfigName.like(format!("%{}%", keyword)))
                .add(sys_config::Column::ConfigKey.like(format!("%{}%", keyword))),
        );
    }
    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_config::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_config::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_config::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_config::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_config::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_config::Column::UpdatedAt.lte(v));
    }
    let select = sys_config::Entity::find()
        .filter(cond)
        .filter(sys_config::Column::DeletedAt.is_null())
        // 统一按主键降序：与其余分页域一致，且 id 唯一且稳定，分页边界确定。
        // （原按 CreatedAt 倒序：同一秒内多行时顺序仍不确定）
        .order_by_desc(sys_config::Column::Id);

    let page = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page)
}

/// 参数：按 config_key 查记录——**含软删占位**，不过滤 deleted_at。
/// 供创建/更新查重：键名删除后仍占位，防止历史引用歧义。
pub async fn find_config_by_key_include_deleted(
    db: &impl ConnectionTrait,
    key: &str,
) -> anyhow::Result<Option<sys_config::Model>> {
    let model = sys_config::Entity::find()
        .filter(sys_config::Column::ConfigKey.eq(key))
        .one(db)
        .await?;
    Ok(model)
}

/// 参数：创建。`actor_id` 为操作人，审计字段由 repo 统一盖章双写。
pub async fn create_config(
    db: &impl ConnectionTrait,
    model: sys_config::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<sys_config::Model> {
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    let model = model.insert(db).await?;
    Ok(model)
}

/// 参数：更新（主键必须已设置）。审计字段由 repo 统一盖章：只刷新更新人。
pub async fn update_config(
    db: &impl ConnectionTrait,
    model: sys_config::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<sys_config::Model> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    let model = model.update(db).await?;
    Ok(model)
}

/// 参数：软删单条，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_config(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let model = find_config_by_id(db, id).await?;
    if let Some(model) = model {
        let mut model: sys_config::ActiveModel = model.into();
        model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        model.update(db).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// —— 网站设置 ——

/// 网站设置：查单行（恒 id=1）。
pub async fn find_site_config(
    db: &impl ConnectionTrait,
) -> anyhow::Result<Option<sys_site_config::Model>> {
    let model = sys_site_config::Entity::find()
        .filter(sys_site_config::Column::Id.eq(1))
        .one(db)
        .await?;
    Ok(model)
}

/// 网站设置：更新单行（主键必须已设置 id=1）。审计字段由 repo 统一盖章：只刷新更新人。
pub async fn update_site_config(
    db: &impl ConnectionTrait,
    model: sys_site_config::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<sys_site_config::Model> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    let model = model.update(db).await?;
    Ok(model)
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

    /// 造一条参数记录；key 用唯一名防测试间冲突。
    async fn seed_config(
        db: &impl ConnectionTrait,
        key: &str,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_config::Model {
        sys_config::ActiveModel {
            config_name: Set(format!("参数{key}")),
            config_key: Set(key.to_string()),
            config_value: Set("v".to_string()),
            remark: Set(String::new()),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_config_page_filters_by_keyword_sorts_desc_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("repo_cfg");
        let base = chrono::Local::now().naive_local();

        // 插入顺序（id 升序）与 created_at 新旧相反：a 最新 id 最小，
        // 「按 created_at 倒序」得 [a,b,c]，「按 id 倒序」得 [c,b,a]，两实现可区分。
        let a = seed_config(&db, &format!("{kw}a"), base, None).await;
        let b = seed_config(
            &db,
            &format!("{kw}b"),
            base - chrono::Duration::seconds(2),
            None,
        )
        .await;
        let c = seed_config(
            &db,
            &format!("{kw}c"),
            base - chrono::Duration::seconds(3),
            None,
        )
        .await;
        let _deleted = seed_config(
            &db,
            &format!("{kw}d"),
            base,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let page = find_config_page(
            &db,
            &ConfigFilter {
                keyword: Some(kw),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(page.total, 3, "keyword 应命中 3 条活记录，软删不进分页");
        let ids: Vec<u64> = page.items.iter().map(|m| m.id).collect();
        // 统一口径：分页按主键降序（不再按 created_at）。
        // fixture 刻意让 created_at 序与 id 序相反（a 最新但 id 最小）。
        assert_eq!(
            ids,
            vec![c.id, b.id, a.id],
            "分页应统一按 id 降序（按 created_at 的错误实现会得到 a,b,c）"
        );
    }

    /// 审计字段过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_config_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique("cfg_audit");
        let base = chrono::Local::now().naive_local();
        let a = seed_config(&db, &format!("{kw}a"), base, None).await;
        let b = seed_config(&db, &format!("{kw}b"), base, None).await;

        // update_many 盖不同的审计人与时间：避免为测试改各域 seed 夹具
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_config::Entity::update_many()
                .filter(sys_config::Column::Id.eq(row))
                .col_expr(sys_config::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_config::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_config::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_config::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let by_creator = find_config_page(
            &db,
            &ConfigFilter {
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

        let by_time = find_config_page(
            &db,
            &ConfigFilter {
                keyword: Some(kw.clone()),
                updated_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_time.total, 1, "updated_at begin=base-7s 应只剩 b");
    }

    /// 审计过滤不依赖 keyword：仅传 created_by（不传 keyword）也应生效。
    #[tokio::test]
    async fn find_config_page_filters_by_audit_columns_without_keyword() {
        let db = test_txn().await;
        let a = seed_config(
            &db,
            &unique("cfg_nokw_a"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;
        let b = seed_config(
            &db,
            &unique("cfg_nokw_b"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;

        // update_many 盖不同的审计人：避免为测试改各域 seed 夹具
        for (row, by) in [(a.id, 7_i64), (b.id, 8_i64)] {
            sys_config::Entity::update_many()
                .filter(sys_config::Column::Id.eq(row))
                .col_expr(sys_config::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_config::Column::UpdatedBy, Expr::value(by))
                .exec(&db)
                .await
                .unwrap();
        }

        let by_creator = find_config_page(
            &db,
            &ConfigFilter {
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

    #[tokio::test]
    async fn find_config_by_key_includes_soft_deleted_placeholder() {
        let db = test_txn().await;
        let key = unique("cfg_key");
        let _deleted = seed_config(
            &db,
            &key,
            chrono::Local::now().naive_local(),
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let hit = find_config_by_key_include_deleted(&db, &key).await.unwrap();

        assert!(
            hit.is_some(),
            "查重必须含软删占位：键名删除后仍应视为已占用"
        );
    }

    #[tokio::test]
    async fn soft_delete_config_marks_deleted_and_second_call_returns_false() {
        let db = test_txn().await;
        let a = seed_config(
            &db,
            &unique("cfg_del"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;

        let first = soft_delete_config(&db, a.id).await.unwrap();
        let after = find_config_by_id(&db, a.id).await.unwrap();
        let second = soft_delete_config(&db, a.id).await.unwrap();

        assert!(first, "首次软删应返回 true");
        assert!(after.is_none(), "软删后 find_config_by_id 不可见");
        assert!(!second, "重复软删应返回 false");
    }

    #[tokio::test]
    async fn site_config_seed_row_exists_and_update_succeeds() {
        let db = test_txn().await;

        let before = find_site_config(&db).await.unwrap();
        assert!(before.is_some(), "迁移种子行应存在（id=1）");
        assert_eq!(before.unwrap().id, 1);

        // 更新：主键已设置（从查到的 Model 转 ActiveModel），走 repo 盖章路径
        let model = find_site_config(&db).await.unwrap().unwrap();
        let mut am: sys_site_config::ActiveModel = model.into();
        am.name = Set("迁移验证临时名".to_string());
        let updated = update_site_config(&db, am, 0).await.unwrap();

        // 恢复种子行的展示名，避免污染默认站点设置
        let model = find_site_config(&db).await.unwrap().unwrap();
        let mut am: sys_site_config::ActiveModel = model.into();
        am.name = Set("tide-server".to_string());
        update_site_config(&db, am, 0).await.unwrap();

        assert_eq!(updated.name, "迁移验证临时名");
    }
}
