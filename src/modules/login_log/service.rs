//! 登录日志业务（codegen 生成后裁剪：只读 + 删除 + 批量删除）。

use sea_orm::DatabaseConnection;

use crate::entity::sys_login_log;
use crate::modules::login_log::dto::LoginLogListReq;
use crate::modules::login_log::repo as login_log_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 分页查询。
pub async fn page_login_logs(
    db: &DatabaseConnection,
    req: &LoginLogListReq,
) -> anyhow::Result<PageData<sys_login_log::Model>> {
    login_log_repo::find_page(
        db,
        &crate::modules::login_log::dto::LoginLogFilter {
            username: req.username.clone(),
            ip: req.ip.clone(),
            status: req.status,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 查询单个详情（排除软删除）。
pub async fn get_login_log(
    db: &DatabaseConnection,
    id: u64,
) -> Result<sys_login_log::Model, AppError> {
    let Some(model) = login_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("登录日志不存在：{id}")));
    };
    Ok(model)
}

/// 删除：判存在后软删。
pub async fn delete_login_log(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let Some(_) = login_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("登录日志不存在：{id}")));
    };
    login_log_repo::soft_delete_login_log(db, id).await?;
    Ok(())
}

/// 批量软删：空数组返回 0；repo 层忽略不存在的 id。
pub async fn delete_login_log_batch(db: &DatabaseConnection, ids: &[u64]) -> Result<u64, AppError> {
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(login_log_repo::soft_delete_batch(db, ids).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_login_log;
    use crate::utils::PageQuery;
    use crate::utils::error::AppError;
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

    #[allow(clippy::too_many_arguments)]
    async fn seed(
        db: &DatabaseConnection,
        username: &str,
        ip: &str,
        status: i8,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_login_log::Model {
        sys_login_log::ActiveModel {
            user_id: Set(1),
            username: Set(username.to_string()),
            ip: Set(ip.to_string()),
            agent: Set("test-agent".to_string()),
            status: Set(status),
            msg: Set("登录成功".to_string()),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn list_req(
        username: Option<String>,
        ip: Option<String>,
        status: Option<i8>,
    ) -> LoginLogListReq {
        LoginLogListReq {
            page: PageQuery {
                page: None,
                page_size: None,
            },
            username,
            ip,
            status,
        }
    }

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_login_log::Entity::delete_many()
            .filter(sys_login_log::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn page_login_logs_filters_by_username_ip_status() {
        let db = test_db().await;
        let kw = unique("svc_page");
        let base = chrono::Local::now().naive_local();
        let deleted_at = Some(chrono::Local::now().naive_local());

        let hit = seed(&db, &format!("svc{kw}a"), "10.0.0.1", 1, base, None).await;
        let other_status = seed(
            &db,
            &format!("svc{kw}b"),
            "10.0.0.1",
            0,
            base - chrono::Duration::seconds(1),
            None,
        )
        .await;
        let other_ip = seed(
            &db,
            &format!("svc{kw}c"),
            "10.0.0.2",
            1,
            base - chrono::Duration::seconds(2),
            None,
        )
        .await;
        let deleted = seed(
            &db,
            &format!("svc{kw}d"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(3),
            deleted_at,
        )
        .await;

        let by_username = page_login_logs(&db, &list_req(Some(kw.clone()), None, None))
            .await
            .unwrap();
        let by_ip_status = page_login_logs(
            &db,
            &list_req(Some(kw.clone()), Some("10.0.0.1".to_string()), Some(1)),
        )
        .await
        .unwrap();

        cleanup(&db, &[hit.id, other_status.id, other_ip.id, deleted.id]).await;

        assert_eq!(by_username.total, 3, "keyword 应命中 3 条活记录");
        assert_eq!(by_ip_status.total, 1, "ip + status 精确过滤");
        assert_eq!(by_ip_status.items[0].id, hit.id);
    }

    #[tokio::test]
    async fn get_and_delete_missing_return_biz_error() {
        let db = test_db().await;

        let get_missing = get_login_log(&db, 9_999_999_999).await;
        let delete_missing = delete_login_log(&db, 9_999_999_999).await;

        assert!(matches!(get_missing, Err(AppError::Biz(_))));
        assert!(matches!(delete_missing, Err(AppError::Biz(_))));
    }

    #[tokio::test]
    async fn delete_batch_skips_missing_and_empty_ok() {
        let db = test_db().await;
        let kw = unique("svc_batch");
        let base = chrono::Local::now().naive_local();
        let a = seed(&db, &format!("svc{kw}a"), "10.0.0.1", 1, base, None).await;
        let b = seed(
            &db,
            &format!("svc{kw}b"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(1),
            None,
        )
        .await;
        let already_deleted = seed(
            &db,
            &format!("svc{kw}c"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(2),
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let empty = delete_login_log_batch(&db, &[]).await.unwrap();
        let affected =
            delete_login_log_batch(&db, &[a.id, b.id, already_deleted.id, 9_999_999_999])
                .await
                .unwrap();
        let a_after = login_log_repo::find_by_id(&db, a.id).await.unwrap();
        let b_after = login_log_repo::find_by_id(&db, b.id).await.unwrap();

        cleanup(&db, &[a.id, b.id, already_deleted.id]).await;

        assert_eq!(empty, 0, "空数组应返回 0 且不执行删除");
        assert_eq!(affected, 2, "只处理存在且未删除的行");
        assert!(a_after.is_none() && b_after.is_none(), "批量软删后不可见");
    }
}
