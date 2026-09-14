//! 操作日志业务（codegen 生成）。

use sea_orm::ConnectionTrait;

use crate::entity::sys_operation_log;
use crate::modules::system::operation_log::dto::{OperationLogFilter, OperationLogListReq};
use crate::modules::system::operation_log::repo as operation_log_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 分页查询：请求参数透传 repo 过滤条件。
pub async fn page_operation_logs(
    db: &impl ConnectionTrait,
    req: &OperationLogListReq,
) -> anyhow::Result<PageData<sys_operation_log::Model>> {
    operation_log_repo::find_page(
        db,
        &OperationLogFilter {
            user_id: req.user_id,
            status: req.status,
            keyword: req.keyword.clone(),
            ip: req.ip.clone(),
            created_at_begin: req.created_at_begin.clone(),
            created_at_end: req.created_at_end.clone(),
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 查询单个详情（排除软删除）。
pub async fn get_operation_log(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<sys_operation_log::Model, AppError> {
    let Some(model) = operation_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("操作日志不存在：{id}")));
    };
    Ok(model)
}

/// 删除：判存在后软删。
pub async fn delete_operation_log(db: &impl ConnectionTrait, id: u64) -> Result<(), AppError> {
    let Some(_) = operation_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("操作日志不存在：{id}")));
    };
    operation_log_repo::soft_delete_operation_log(db, id).await?;
    Ok(())
}

/// 批量软删：空数组返回 0；repo 层忽略不存在的 id。
pub async fn delete_operation_log_batch(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> Result<u64, AppError> {
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(operation_log_repo::soft_delete_batch(db, ids).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_operation_log;
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

    fn list_req(
        keyword: Option<String>,
        user_id: Option<u64>,
        status: Option<i32>,
    ) -> OperationLogListReq {
        OperationLogListReq {
            page: PageQuery {
                page: None,
                page_size: None,
            },
            user_id,
            status,
            keyword,
            ip: None,
            created_at_begin: None,
            created_at_end: None,
        }
    }

    #[tokio::test]
    async fn page_operation_logs_filters_by_keyword_user_and_status() {
        let db = test_txn().await;
        let kw = unique("svc_page");
        let base = chrono::Local::now().naive_local();
        let deleted_at = Some(chrono::Local::now().naive_local());

        let _hit = seed(&db, &kw, 1, 200, base, None).await;
        let other_user = seed(&db, &kw, 2, 200, base - chrono::Duration::seconds(1), None).await;
        let other_status = seed(&db, &kw, 1, 500, base - chrono::Duration::seconds(2), None).await;
        let _deleted = seed(
            &db,
            &kw,
            1,
            200,
            base - chrono::Duration::seconds(3),
            deleted_at,
        )
        .await;

        let by_keyword = page_operation_logs(&db, &list_req(Some(kw.clone()), None, None))
            .await
            .unwrap();
        let by_user = page_operation_logs(&db, &list_req(Some(kw.clone()), Some(2), None))
            .await
            .unwrap();
        let by_status = page_operation_logs(&db, &list_req(Some(kw.clone()), None, Some(500)))
            .await
            .unwrap();

        assert_eq!(by_keyword.total, 3, "keyword 应命中 3 条活记录");
        assert_eq!(by_user.total, 1, "user_id 精确过滤");
        assert_eq!(by_user.items[0].id, other_user.id);
        assert_eq!(by_status.total, 1, "status 精确过滤");
        assert_eq!(by_status.items[0].id, other_status.id);
    }

    #[tokio::test]
    async fn get_and_delete_missing_return_biz_error() {
        let db = test_txn().await;

        let get_missing = get_operation_log(&db, 9_999_999_999).await;
        let delete_missing = delete_operation_log(&db, 9_999_999_999).await;

        assert!(matches!(get_missing, Err(AppError::Biz(_))));
        assert!(matches!(delete_missing, Err(AppError::Biz(_))));
    }

    #[tokio::test]
    async fn delete_batch_skips_missing_and_empty_ok() {
        let db = test_txn().await;
        let kw = unique("svc_batch");
        let base = chrono::Local::now().naive_local();
        let a = seed(&db, &kw, 1, 200, base, None).await;
        let b = seed(&db, &kw, 1, 200, base - chrono::Duration::seconds(1), None).await;
        let already_deleted = seed(
            &db,
            &kw,
            1,
            200,
            base - chrono::Duration::seconds(2),
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let empty = delete_operation_log_batch(&db, &[]).await.unwrap();
        let affected =
            delete_operation_log_batch(&db, &[a.id, b.id, already_deleted.id, 9_999_999_999])
                .await
                .unwrap();
        let a_after = operation_log_repo::find_by_id(&db, a.id).await.unwrap();
        let b_after = operation_log_repo::find_by_id(&db, b.id).await.unwrap();

        assert_eq!(empty, 0, "空数组应返回 0 且不执行删除");
        assert_eq!(affected, 2, "只处理存在且未删除的行");
        assert!(a_after.is_none() && b_after.is_none(), "批量软删后不可见");
    }
}
