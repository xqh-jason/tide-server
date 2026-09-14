//! 定时任务执行日志业务（codegen 生成后裁剪：只读 + 删除 + 批量删除）。

use sea_orm::ConnectionTrait;

use crate::entity::sys_job_log;
use crate::modules::system::job_log::dto::JobLogListReq;
use crate::modules::system::job_log::repo as job_log_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 分页查询：请求参数透传 repo 过滤条件。
pub async fn page_job_logs(
    db: &impl ConnectionTrait,
    req: &JobLogListReq,
) -> anyhow::Result<PageData<sys_job_log::Model>> {
    job_log_repo::find_page(
        db,
        &crate::modules::system::job_log::dto::JobLogFilter {
            job_id: req.job_id,
            status: req.status,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 查询单个详情（排除软删除）。
pub async fn get_job_log(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<sys_job_log::Model, AppError> {
    let Some(model) = job_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("执行日志不存在：{id}")));
    };
    Ok(model)
}

/// 删除：判存在后软删。
pub async fn delete_job_log(db: &impl ConnectionTrait, id: u64) -> Result<(), AppError> {
    let Some(_) = job_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("执行日志不存在：{id}")));
    };
    job_log_repo::soft_delete_job_log(db, id).await?;
    Ok(())
}

/// 批量软删：空数组返回 0；repo 层忽略不存在的 id。
pub async fn delete_job_log_batch(db: &impl ConnectionTrait, ids: &[u64]) -> Result<u64, AppError> {
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(job_log_repo::soft_delete_batch(db, ids).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_job_log;
    use crate::utils::PageQuery;
    use crate::utils::error::AppError;
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
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

    async fn seed(
        db: &impl ConnectionTrait,
        job_id: u64,
        status: i8,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_job_log::Model {
        sys_job_log::ActiveModel {
            job_id: Set(job_id),
            job_name: Set(unique("svc_job_log")),
            status: Set(status),
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

    fn list_req(job_id: Option<u64>, status: Option<i8>) -> JobLogListReq {
        JobLogListReq {
            page: PageQuery {
                page: None,
                page_size: None,
            },
            job_id,
            status,
        }
    }

    #[tokio::test]
    async fn page_job_logs_filters_by_job_id_and_status() {
        let db = test_txn().await;
        let base = chrono::Local::now().naive_local();

        let hit = seed(&db, 101, 1, base, None).await;
        let _other_status = seed(&db, 101, 0, base - chrono::Duration::seconds(1), None).await;
        let _other_job = seed(&db, 202, 1, base - chrono::Duration::seconds(2), None).await;
        let _deleted = seed(
            &db,
            101,
            1,
            base - chrono::Duration::seconds(3),
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let by_job = page_job_logs(&db, &list_req(Some(101), None))
            .await
            .unwrap();
        let by_job_status = page_job_logs(&db, &list_req(Some(101), Some(1)))
            .await
            .unwrap();

        assert_eq!(by_job.total, 2, "job_id 精确过滤应命中 2 条活记录");
        assert_eq!(by_job_status.total, 1, "job_id + status 精确过滤");
        assert_eq!(by_job_status.items[0].id, hit.id, "created_at 倒序");
    }

    #[tokio::test]
    async fn get_and_delete_missing_return_biz_error() {
        let db = test_txn().await;

        let get_missing = get_job_log(&db, 9_999_999_999).await;
        let delete_missing = delete_job_log(&db, 9_999_999_999).await;

        assert!(matches!(get_missing, Err(AppError::Biz(_))));
        assert!(matches!(delete_missing, Err(AppError::Biz(_))));
    }

    #[tokio::test]
    async fn delete_batch_skips_missing_and_empty_ok() {
        let db = test_txn().await;
        let base = chrono::Local::now().naive_local();
        let a = seed(&db, 301, 1, base, None).await;
        let b = seed(&db, 302, 1, base - chrono::Duration::seconds(1), None).await;
        let already_deleted = seed(
            &db,
            303,
            1,
            base - chrono::Duration::seconds(2),
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let empty = delete_job_log_batch(&db, &[]).await.unwrap();
        let affected = delete_job_log_batch(&db, &[a.id, b.id, already_deleted.id, 9_999_999_999])
            .await
            .unwrap();
        let a_after = job_log_repo::find_by_id(&db, a.id).await.unwrap();
        let b_after = job_log_repo::find_by_id(&db, b.id).await.unwrap();

        assert_eq!(empty, 0, "空数组应返回 0 且不执行删除");
        assert_eq!(affected, 2, "只处理存在且未删除的行");
        assert!(a_after.is_none() && b_after.is_none(), "批量软删后不可见");
    }
}
