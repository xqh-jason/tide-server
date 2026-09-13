//! 文件数据访问（`sys_file`）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, QueryOrder};

use crate::entity::{sys_file, sys_file::Model};
use crate::modules::file::dto::FileFilter;

/// 按主键查有效记录（排除软删）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let model = sys_file::Entity::find()
        .filter(sys_file::Column::Id.eq(id))
        .filter(sys_file::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 分页 + 动态过滤（keyword 对 name 模糊），排除软删，created_at 倒序。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &FileFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = filter.keyword.as_deref() {
        cond = cond.add(sys_file::Column::Name.like(format!("%{}%", keyword)));
    }
    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_file::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_file::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_file::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_file::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_file::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_file::Column::UpdatedAt.lte(v));
    }

    let select = sys_file::Entity::find()
        .filter(cond)
        .filter(sys_file::Column::DeletedAt.is_null())
        .order_by_desc(sys_file::Column::CreatedAt);

    let page = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page)
}

/// 创建记录（上传落库入口）。`actor_id` 为上传人，审计字段由 repo 统一盖章。
pub async fn create_file(
    db: &impl ConnectionTrait,
    model: sys_file::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Model> {
    // 创建场景：创建人与更新人同源（写入口径见 AGENTS.md「人字段命名与名称拼装约定」）
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    let model = model.insert(db).await?;
    Ok(model)
}

/// 软删单条：`deleted_at` 置为当前时间，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_file(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    if let Some(model) = find_by_id(db, id).await? {
        let mut mode: sys_file::ActiveModel = model.into();
        mode.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        mode.update(db).await?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_file;
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

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 造一条文件记录；`stored_name` 用唯一名防测试间冲突（真实 uuid 命名由 service 测试覆盖）。
    #[allow(clippy::too_many_arguments)]
    async fn seed(
        db: &impl ConnectionTrait,
        name: &str,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_file::Model {
        let stored_name = format!("{}.txt", unique("stored"));
        sys_file::ActiveModel {
            name: Set(name.to_string()),
            stored_name: Set(stored_name),
            ext: Set("txt".to_string()),
            mime: Set("text/plain".to_string()),
            size: Set(1024),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_page_filters_by_keyword_sorts_desc_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("repo_page");
        let base = chrono::Local::now().naive_local();

        // 插入顺序（id 升序）与 created_at 新旧刻意相反：a 最新但 id 最小。
        // 这样「按 created_at 倒序」得到 [a,b,c]，「按 id 倒序」得到 [c,b,a]，
        // 两种实现结果不同，测试才能区分规格要求的排序字段。
        let a = seed(&db, &format!("报告{kw}alpha.txt"), base, None).await;
        let b = seed(
            &db,
            &format!("报告{kw}beta.txt"),
            base - chrono::Duration::seconds(2),
            None,
        )
        .await;
        let c = seed(
            &db,
            &format!("报告{kw}gamma.txt"),
            base - chrono::Duration::seconds(3),
            None,
        )
        .await;
        let _deleted = seed(
            &db,
            &format!("报告{kw}delta.txt"),
            base,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let _other = seed(&db, "其他文件.txt", base, None).await;

        let page = find_page(
            &db,
            &FileFilter {
                keyword: Some(kw),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(
            page.total, 3,
            "keyword 应只命中 3 条活记录，软删与他名记录不进分页"
        );
        let ids: Vec<u64> = page.items.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec![a.id, b.id, c.id],
            "应按 created_at 倒序（a 最新 id 最小，按 id 倒序的错误实现会得到 c,b,a）"
        );
    }

    #[tokio::test]
    async fn soft_delete_file_marks_deleted_and_second_call_returns_false() {
        let db = test_txn().await;
        let kw = unique("repo_del");
        let a = seed(
            &db,
            &format!("{kw}.txt"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;

        let first = soft_delete_file(&db, a.id).await.unwrap();
        let after = find_by_id(&db, a.id).await.unwrap();
        let second = soft_delete_file(&db, a.id).await.unwrap();

        assert!(first, "首次软删应返回 true");
        assert!(after.is_none(), "软删后 find_by_id 不可见");
        assert!(!second, "重复软删应返回 false");
    }

    /// 审计过滤不依赖 keyword：仅传 created_by（不传 keyword）也应生效。
    #[tokio::test]
    async fn find_page_filters_by_audit_columns_without_keyword() {
        let db = test_txn().await;
        let a = seed(
            &db,
            &format!("{}.txt", unique("audit_nokw_a")),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;
        let b = seed(
            &db,
            &format!("{}.txt", unique("audit_nokw_b")),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;

        // update_many 盖不同的审计人：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by) in [(a.id, 7_i64), (b.id, 8_i64)] {
            sys_file::Entity::update_many()
                .filter(sys_file::Column::Id.eq(row))
                .col_expr(sys_file::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_file::Column::UpdatedBy, Expr::value(by))
                .exec(&db)
                .await
                .unwrap();
        }

        let by_creator = find_page(
            &db,
            &FileFilter {
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
        let a = seed(
            &db,
            &format!("{kw}a.txt"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;
        let b = seed(
            &db,
            &format!("{kw}b.txt"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;

        let base = chrono::Local::now().naive_local();
        // update_many 盖不同的审计人与时间：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_file::Entity::update_many()
                .filter(sys_file::Column::Id.eq(row))
                .col_expr(sys_file::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_file::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_file::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_file::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let all = find_page(
            &db,
            &FileFilter {
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
            &FileFilter {
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
            &FileFilter {
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
            &FileFilter {
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
            &FileFilter {
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
            &FileFilter {
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
            &FileFilter {
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
            &FileFilter {
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

        sys_file::Entity::delete_many()
            .filter(sys_file::Column::Id.is_in([a.id, b.id]))
            .exec(&db)
            .await
            .unwrap();
    }
}
