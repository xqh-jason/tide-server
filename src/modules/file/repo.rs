//! 文件数据访问（`sys_file`）。
//!
//! 规格依据：docs/superpowers/specs/2026-09-05-w5-file-upload-design.md §3 / §8.1。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, QueryOrder};

use crate::entity::{sys_file, sys_file::Model, sys_file_chunk};
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

// ── W6-3 断点续传：分片记录（sys_file_chunk，硬删不软删） ──

/// 分片 upsert：同 (file_md5, chunk_number) 命中则覆盖 chunk_path（幂等重传语义），
/// 未命中插入。唯一键 `uk_sys_file_chunk_file_md5_chunk_number` 兜底并发。
///
/// 实现步骤：
/// 1. 按 `FileMd5.eq(file_md5) + ChunkNumber.eq(chunk_number)` 查现有记录；
/// 2. 命中：转 ActiveModel 覆盖 `chunk_path` 后 update；
/// 3. 未命中：构造 ActiveModel（created_at 保持 NotSet 交数据库默认值）insert。
pub async fn upsert_chunk(
    db: &impl ConnectionTrait,
    file_md5: &str,
    chunk_number: u32,
    chunk_path: &str,
) -> anyhow::Result<()> {
    let _ = (db, file_md5, chunk_number, chunk_path);
    todo!("W6-3 任务 4：分片 upsert")
}

/// 该会话已传分片序号列表，按 chunk_number 升序。
///
/// 实现步骤：Entity::find + filter(FileMd5.eq) + order_by_asc(ChunkNumber)，
/// 只取 chunk_number（select_only().column() 或取整行后 map 均可）。
pub async fn find_chunk_numbers_by_md5(
    db: &impl ConnectionTrait,
    file_md5: &str,
) -> anyhow::Result<Vec<u32>> {
    let _ = (db, file_md5);
    todo!("W6-3 任务 4：已传分片序号查询")
}

/// 硬删该会话全部分片记录，返回删除行数（合并成功 / 放弃上传 / 定时清理共用）。
pub async fn delete_chunks_by_md5(
    db: &impl ConnectionTrait,
    file_md5: &str,
) -> anyhow::Result<u64> {
    let _ = (db, file_md5);
    todo!("W6-3 任务 4：分片硬删")
}

/// 秒传查询：md5 命中未删文件记录（存量记录 md5 为 NULL 天然不命中，无需特判）。
pub async fn find_file_by_md5(
    db: &impl ConnectionTrait,
    file_md5: &str,
) -> anyhow::Result<Option<Model>> {
    let _ = (db, file_md5);
    todo!("W6-3 任务 4：秒传查询")
}

/// 定时清理用：created_at 早于 cutoff 的分片所属 md5 去重列表。
///
/// 实现步骤：filter(CreatedAt.lt(cutoff)) → 只取 FileMd5 → 去重
/// （select_only + column 或取整行 map 后 Rust 侧 dedup 均可，行数有限）。
pub async fn find_expired_chunk_md5s(
    db: &impl ConnectionTrait,
    cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<Vec<String>> {
    let _ = (db, cutoff);
    todo!("W6-3 任务 4：过期分片会话查询")
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

    // ── W6-3 断点续传：分片 repo 测试（sys_file_chunk 真删，唯一 md5 造数防互扰） ──

    use crate::entity::sys_file_chunk;

    /// 唯一 32 位小写 hex md5（SEQ 自增零填充，格式与真实 md5 同形，防测试间互扰）。
    fn unique_chunk_md5() -> String {
        format!("{:032x}", SEQ.fetch_add(1, Ordering::Relaxed) as u128)
    }

    /// 造一条分片记录；created_at 可控（过期测试需要）。
    async fn seed_chunk(
        db: &impl ConnectionTrait,
        file_md5: &str,
        chunk_number: u32,
        chunk_path: &str,
        created_at: chrono::NaiveDateTime,
    ) -> sys_file_chunk::Model {
        sys_file_chunk::ActiveModel {
            file_md5: Set(file_md5.to_string()),
            chunk_number: Set(chunk_number),
            chunk_path: Set(chunk_path.to_string()),
            created_at: Set(created_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn upsert_chunk_overwrites_path_without_duplication() {
        let db = test_txn().await;
        let md5 = unique_chunk_md5();

        upsert_chunk(&db, &md5, 0, "chunks/a/00000.part")
            .await
            .unwrap();
        upsert_chunk(&db, &md5, 0, "chunks/a/00000.part.retried")
            .await
            .unwrap();

        let rows = sys_file_chunk::Entity::find()
            .filter(sys_file_chunk::Column::FileMd5.eq(&md5))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "同 (md5, number) 二次 upsert 行数不翻倍");
        assert_eq!(
            rows[0].chunk_path, "chunks/a/00000.part.retried",
            "重传应覆盖 chunk_path（幂等语义）"
        );
    }

    #[tokio::test]
    async fn find_chunk_numbers_by_md5_returns_sorted_and_scoped() {
        let db = test_txn().await;
        let md5 = unique_chunk_md5();
        let other = unique_chunk_md5();
        let now = chrono::Local::now().naive_local();

        seed_chunk(&db, &md5, 2, "p2", now).await;
        seed_chunk(&db, &md5, 0, "p0", now).await;
        seed_chunk(&db, &md5, 1, "p1", now).await;
        seed_chunk(&db, &other, 5, "p5", now).await;

        let numbers = find_chunk_numbers_by_md5(&db, &md5).await.unwrap();
        assert_eq!(
            numbers,
            vec![0, 1, 2],
            "只含该 md5 且按 chunk_number 升序（2,0,1 插入序打乱）"
        );
    }

    #[tokio::test]
    async fn delete_chunks_by_md5_clears_rows() {
        let db = test_txn().await;
        let md5 = unique_chunk_md5();
        let now = chrono::Local::now().naive_local();
        seed_chunk(&db, &md5, 0, "p0", now).await;
        seed_chunk(&db, &md5, 1, "p1", now).await;

        let deleted = delete_chunks_by_md5(&db, &md5).await.unwrap();
        let left = find_chunk_numbers_by_md5(&db, &md5).await.unwrap();
        let again = delete_chunks_by_md5(&db, &md5).await.unwrap();

        assert_eq!(deleted, 2, "应删除该会话 2 条分片");
        assert!(left.is_empty(), "删除后该 md5 无残留记录");
        assert_eq!(again, 0, "重复删除返回 0（幂等）");
    }

    #[tokio::test]
    async fn find_file_by_md5_hits_undeleted_only() {
        let db = test_txn().await;
        let md5 = unique_chunk_md5();
        let now = chrono::Local::now().naive_local();
        let live = seed(&db, &format!("{md5}live.txt"), now, None).await;
        let _dead = seed(&db, &format!("{md5}dead.txt"), now, Some(now)).await;

        // seed 夹具不写 md5（NotSet → NULL），用 update_many 分别盖同值 md5
        use sea_orm::sea_query::Expr;
        for id in [live.id, _dead.id] {
            sys_file::Entity::update_many()
                .filter(sys_file::Column::Id.eq(id))
                .col_expr(sys_file::Column::Md5, Expr::value(md5.as_str()))
                .exec(&db)
                .await
                .unwrap();
        }

        let hit = find_file_by_md5(&db, &md5).await.unwrap();
        assert_eq!(
            hit.map(|m| m.id),
            Some(live.id),
            "md5 命中未删记录；软删行（同 md5）不得命中"
        );
        let miss = find_file_by_md5(&db, &unique_chunk_md5()).await.unwrap();
        assert!(miss.is_none(), "无命中返回 None");
    }

    #[tokio::test]
    async fn find_expired_chunk_md5s_returns_old_only() {
        let db = test_txn().await;
        let old_md5 = unique_chunk_md5();
        let fresh_md5 = unique_chunk_md5();
        let now = chrono::Local::now().naive_local();

        seed_chunk(&db, &old_md5, 0, "p0", now - chrono::Duration::hours(2)).await;
        seed_chunk(&db, &old_md5, 1, "p1", now - chrono::Duration::hours(2)).await;
        seed_chunk(&db, &fresh_md5, 0, "p0", now).await;

        let expired = find_expired_chunk_md5s(&db, now - chrono::Duration::hours(1))
            .await
            .unwrap();
        assert!(expired.contains(&old_md5), "过期会话应在列：{expired:?}");
        assert!(
            !expired.contains(&fresh_md5),
            "未过期会话不得在列：{expired:?}"
        );
        assert_eq!(
            expired.iter().filter(|m| **m == old_md5).count(),
            1,
            "同会话多片应去重为一个 md5"
        );
    }
}
