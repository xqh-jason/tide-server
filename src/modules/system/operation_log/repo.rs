//! 操作日志数据访问（codegen 生成）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, QueryOrder, QuerySelect};

use crate::entity::{sys_operation_log, sys_operation_log::Model};
use crate::modules::system::operation_log::dto::OperationLogFilter;

/// 查询单个有效记录（排除软删除）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let m = sys_operation_log::Entity::find()
        .filter(sys_operation_log::Column::Id.eq(id))
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(m)
}

/// 分页 + 动态过滤查询（user_id / status 精确，path 模糊），created_at 倒序。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &OperationLogFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(v) = &filter.user_id {
        cond = cond.add(sys_operation_log::Column::UserId.eq(*v));
    }
    if let Some(v) = &filter.status {
        cond = cond.add(sys_operation_log::Column::Status.eq(*v));
    }

    if let Some(v) = &filter.keyword {
        // 前缀匹配（`x%`）才能走索引；前导通配符（`%x%`）会让 B-Tree 索引失效，
        // 退化成全表扫描（EXPLAIN 的 type=ALL）。
        // 使用方式：输入 `/api/v1/user` 可检索该前缀下所有接口的调用记录。
        cond = cond.add(sys_operation_log::Column::Path.like(format!("{v}%")));
    }
    if let Some(v) = &filter.ip {
        cond = cond.add(sys_operation_log::Column::Ip.like(format!("%{}%", v)));
    }
    if let Some(v) = &filter.created_at_begin {
        cond = cond.add(sys_operation_log::Column::CreatedAt.gte(v));
    }
    if let Some(v) = &filter.created_at_end {
        cond = cond.add(sys_operation_log::Column::CreatedAt.lte(v));
    }

    let select = sys_operation_log::Entity::find()
        .filter(cond)
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        // 统一按主键降序：与其余分页域一致，且 id 唯一且稳定，分页边界确定。
        // （原按 CreatedAt 倒序：同一秒内多行时顺序仍不确定，且无索引时更慢）
        .order_by_desc(sys_operation_log::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建记录。
pub async fn create_operation_log(
    db: &impl ConnectionTrait,
    model: sys_operation_log::ActiveModel,
) -> anyhow::Result<Model> {
    Ok(model.insert(db).await?)
}

/// 软删除：`deleted_at` 置为当前时间。
pub async fn soft_delete_operation_log(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_operation_log::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

/// 批量软删：只处理存在且未删除的行，返回受影响行数。
pub async fn soft_delete_batch(db: &impl ConnectionTrait, ids: &[u64]) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let result = sys_operation_log::Entity::update_many()
        .filter(sys_operation_log::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .col_expr(sys_operation_log::Column::DeletedAt, Expr::value(Some(now)))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

/// 物理删除 created_at 早于 cutoff 的记录（定时清理任务用），返回受影响行数。
pub async fn delete_created_before(
    db: &impl ConnectionTrait,
    cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<u64> {
    const BATCH_SIZE: u64 = 1000;
    let mut total: u64 = 0;

    loop {
        let ids = sys_operation_log::Entity::find()
            .filter(sys_operation_log::Column::CreatedAt.lt(cutoff))
            .order_by_asc(sys_operation_log::Column::Id)
            .limit(BATCH_SIZE)
            .all(db)
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect::<Vec<_>>();

        if ids.is_empty() {
            return Ok(total);
        }

        let result = sys_operation_log::Entity::delete_many()
            .filter(sys_operation_log::Column::Id.is_in(ids))
            .exec(db)
            .await?;
        total += result.rows_affected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;
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

    #[allow(clippy::too_many_arguments)]
    async fn seed(
        db: &impl ConnectionTrait,
        path_kw: &str,
        user_id: u64,
        status: i32,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_operation_log::Model {
        sys_operation_log::ActiveModel {
            user_id: Set(user_id),
            ip: Set("127.0.0.1".to_string()),
            method: Set("POST".to_string()),
            path: Set(format!("/api/v1/test/{path_kw}")),
            status: Set(status),
            latency: Set(10),
            agent: Set("test-agent".to_string()),
            body: Set("{\"password\":\"secret\"}".to_string()),
            resp: Set("{\"code\":1}".to_string()),
            error_message: Set(String::new()),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_page_filters_by_keyword_user_and_status_and_sorts_desc_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("repo_page");
        let base = chrono::Local::now().naive_local();
        let deleted_at = Some(chrono::Local::now().naive_local());

        // 命中 keyword 的三条活记录：用 `/api/v1/test/{kw}` 作 keyword。
        // seed 会把传入值拼成 `/api/v1/test/{值}`，故这些行的 path 均以该串开头，
        // 前缀匹配（LIKE 'x%'）能命中且可走索引（LIKE '%x%' 退化为全表扫）。
        // created_at 依次递增（与插入序一致）。
        let prefix = format!("/api/v1/test/{kw}");
        let older = seed(&db, &kw, 1, 200, base - chrono::Duration::seconds(3), None).await;
        let middle = seed(&db, &kw, 2, 200, base - chrono::Duration::seconds(2), None).await;
        let newer = seed(&db, &kw, 1, 500, base - chrono::Duration::seconds(1), None).await;
        // 命中 keyword 但已软删：任何过滤都不应出现
        let _deleted = seed(&db, &kw, 1, 200, base, deleted_at).await;

        let by_keyword = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(prefix.clone()),
                created_at_begin: None,
                created_at_end: None,
                ip: None,
                status: None,
                user_id: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let by_user = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(prefix.clone()),
                user_id: Some(2),
                status: None,
                created_at_begin: None,
                created_at_end: None,
                ip: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let by_status = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(prefix.clone()),
                user_id: None,
                status: Some(500),
                created_at_begin: None,
                created_at_end: None,
                ip: None,
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(
            by_keyword.total, 3,
            "keyword 前缀应命中 3 条活记录（keyword 现在是路径前缀匹配）"
        );
        let ids: Vec<u64> = by_keyword.items.iter().map(|m| m.id).collect();
        // 统一口径：分页按主键降序。fixture 的插入序为 older→middle→newer，
        // 故 id 降序 = [newer, middle, older]；created_at 序恰好相同，无法区分，
        // 排序语义由 file/config 的专用用例守（那里两序相反）。
        assert_eq!(ids, vec![newer.id, middle.id, older.id], "应按 id 降序");
        assert_eq!(by_user.total, 1, "user_id 精确过滤");
        assert_eq!(by_user.items[0].id, middle.id);
        assert_eq!(by_status.total, 1, "status 精确过滤");
        assert_eq!(by_status.items[0].id, newer.id);
    }

    #[tokio::test]
    async fn soft_delete_batch_only_marks_alive_rows() {
        let db = test_txn().await;
        let kw = unique("repo_batch");
        let base = chrono::Local::now().naive_local();
        let a = seed(&db, &kw, 1, 200, base - chrono::Duration::seconds(2), None).await;
        let b = seed(&db, &kw, 1, 200, base - chrono::Duration::seconds(1), None).await;
        let deleted = seed(
            &db,
            &kw,
            1,
            200,
            base,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let affected = soft_delete_batch(&db, &[a.id, b.id, deleted.id])
            .await
            .unwrap();
        // keyword 为路径前缀（seed 拼成 /api/v1/test/{kw}）
        let prefix = format!("/api/v1/test/{kw}");
        let after = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(prefix.clone()),
                user_id: None,
                status: None,
                created_at_begin: None,
                created_at_end: None,
                ip: None,
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(affected, 2, "已软删记录不应重复计入");
        assert_eq!(after.total, 0, "批量软删后全部不可见");
    }
}
