//! 定时任务执行日志数据访问（codegen 生成后裁剪：只读 + 软删）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, QueryOrder, QuerySelect};

use crate::entity::{sys_job_log, sys_job_log::Model};
use crate::modules::system::job_log::dto::JobLogFilter;

/// 查询单个有效记录（排除软删除）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let m = sys_job_log::Entity::find()
        .filter(sys_job_log::Column::Id.eq(id))
        .filter(sys_job_log::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(m)
}

/// 分页 + 动态过滤查询（job_id / status 精确），排除软删，created_at 倒序。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &JobLogFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(v) = &filter.job_id {
        cond = cond.add(sys_job_log::Column::JobId.eq(*v));
    }
    if let Some(v) = &filter.status {
        cond = cond.add(sys_job_log::Column::Status.eq(*v));
    }

    let select = sys_job_log::Entity::find()
        .filter(cond)
        .filter(sys_job_log::Column::DeletedAt.is_null())
        // 统一按主键降序：与其余分页域一致，且 id 唯一且稳定，分页边界确定。
        // （原按 CreatedAt 倒序：同一秒内多行时顺序仍不确定，且无索引时更慢）
        .order_by_desc(sys_job_log::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 执行日志落库（job_runner 写入入口，无需审计盖章）。
/// 入参为业务字段而非 ActiveModel：调度器（job 域）不直接构造 job_log 域 Entity。
pub async fn create_job_log(
    db: &impl ConnectionTrait,
    job_id: u64,
    job_name: String,
    status: i8,
    error_msg: String,
    duration_ms: u32,
) -> anyhow::Result<Model> {
    let model = sys_job_log::ActiveModel {
        job_id: Set(job_id),
        job_name: Set(job_name),
        status: Set(status),
        error_msg: Set(error_msg),
        duration_ms: Set(duration_ms),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// 软删除：`deleted_at` 置为当前时间。
pub async fn soft_delete_job_log(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_job_log::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

/// 分批物理删除 created_at 早于 cutoff 的记录（定时清理任务用）。
///
/// 滚动取「最旧一批 id → delete_many」循环至无剩余：避免单条 DELETE 命中
/// 海量过期行长时间锁表；已软删行一并物理清除。返回实际删除行数。
pub async fn delete_created_before(
    db: &impl ConnectionTrait,
    cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<u64> {
    const BATCH_SIZE: u64 = 1000;
    let mut total: u64 = 0;
    loop {
        let ids: Vec<u64> = sys_job_log::Entity::find()
            .filter(sys_job_log::Column::CreatedAt.lt(cutoff))
            .order_by_asc(sys_job_log::Column::Id)
            .limit(BATCH_SIZE)
            .all(db)
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect();
        if ids.is_empty() {
            return Ok(total);
        }
        let result = sys_job_log::Entity::delete_many()
            .filter(sys_job_log::Column::Id.is_in(ids))
            .exec(db)
            .await?;
        total += result.rows_affected;
    }
}

/// 批量软删：只处理存在且未删除的行，返回受影响行数。
pub async fn soft_delete_batch(db: &impl ConnectionTrait, ids: &[u64]) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let result = sys_job_log::Entity::update_many()
        .filter(sys_job_log::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_job_log::Column::DeletedAt.is_null())
        .col_expr(sys_job_log::Column::DeletedAt, Expr::value(Some(now)))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;
    use sea_orm::TransactionTrait;
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
        test_db().await.begin().await.unwrap()
    }

    async fn seed(
        db: &impl ConnectionTrait,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> Model {
        sys_job_log::ActiveModel {
            job_id: Set(1),
            job_name: Set(unique("repo_job_log")),
            status: Set(1),
            error_msg: Set(String::new()),
            duration_ms: Set(0),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn all_ids(db: &impl ConnectionTrait) -> Vec<u64> {
        sys_job_log::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect()
    }

    #[tokio::test]
    async fn delete_created_before_removes_expired_rows_including_soft_deleted() {
        let db = test_txn().await;
        let now = chrono::Local::now().naive_local();

        let old_active = seed(&db, now - chrono::Duration::days(200), None).await;
        let old_soft_deleted = seed(&db, now - chrono::Duration::days(200), Some(now)).await;
        let recent_active = seed(&db, now - chrono::Duration::days(1), None).await;
        let recent_soft_deleted = seed(&db, now - chrono::Duration::days(1), Some(now)).await;

        let deleted = delete_created_before(&db, now - chrono::Duration::days(180))
            .await
            .unwrap();

        // 真库可能残留早于 cutoff 的行（测试事务开始前已提交、在快照内），故不断言精确数
        assert!(
            deleted >= 2,
            "过期活跃行与软删行都应被物理删除，实际 {deleted}"
        );
        let ids = all_ids(&db).await;
        assert!(!ids.contains(&old_active.id), "过期活跃行应被删除");
        assert!(
            !ids.contains(&old_soft_deleted.id),
            "过期软删行应一并物理删除"
        );
        assert!(ids.contains(&recent_active.id), "未过期行应保留");
        assert!(ids.contains(&recent_soft_deleted.id), "未过期软删行应保留");
    }
}
