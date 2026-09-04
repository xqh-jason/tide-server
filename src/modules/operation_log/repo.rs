//! 操作日志数据访问（codegen 生成）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, QueryOrder};

use crate::entity::{sys_operation_log, sys_operation_log::Model};
use crate::modules::operation_log::dto::OperationLogFilter;

/// 查询单个有效记录（排除软删除）。
pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    let m = sys_operation_log::Entity::find()
        .filter(sys_operation_log::Column::Id.eq(id))
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(m)
}

/// 分页 + 动态过滤查询（过滤条件由域定义驱动）。
pub async fn find_page(
    db: &DatabaseConnection,
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
        cond = cond.add(sys_operation_log::Column::Path.like(format!("%{}%", v)));
    }

    let select = sys_operation_log::Entity::find()
        .filter(cond)
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .order_by_desc(sys_operation_log::Column::CreatedAt);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建记录。
pub async fn create_operation_log(
    db: &DatabaseConnection,
    model: sys_operation_log::ActiveModel,
) -> anyhow::Result<Model> {
    Ok(model.insert(db).await?)
}

/// 软删除：`deleted_at` 置为当前时间。
pub async fn soft_delete_operation_log(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_operation_log::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

/// 批量软删：只处理存在且未删除的行，返回受影响行数。
pub async fn soft_delete_batch(db: &DatabaseConnection, ids: &[u64]) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let result = sys_operation_log::Entity::update_many()
        .filter(sys_operation_log::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .col_expr(sys_operation_log::Column::DeletedAt, Expr::value(Some(now)))
        .exec(db)
        .await?;

    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_operation_log;
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

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_operation_log::Entity::delete_many()
            .filter(sys_operation_log::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn find_page_filters_by_keyword_user_and_status_and_sorts_desc_excludes_deleted() {
        let db = test_db().await;
        let kw = unique("repo_page");
        let base = chrono::Local::now().naive_local();
        let deleted_at = Some(chrono::Local::now().naive_local());

        // 命中 keyword 的三条活记录，created_at 依次递增（倒序应返回 newest 在前）
        let older = seed(&db, &kw, 1, 200, base - chrono::Duration::seconds(3), None).await;
        let middle = seed(&db, &kw, 2, 200, base - chrono::Duration::seconds(2), None).await;
        let newer = seed(&db, &kw, 1, 500, base - chrono::Duration::seconds(1), None).await;
        // 命中 keyword 但已软删：任何过滤都不应出现
        let deleted = seed(&db, &kw, 1, 200, base, deleted_at).await;

        let by_keyword = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(kw.clone()),
                user_id: None,
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let by_user = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(kw.clone()),
                user_id: Some(2),
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let by_status = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(kw.clone()),
                user_id: None,
                status: Some(500),
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[older.id, middle.id, newer.id, deleted.id]).await;

        assert_eq!(by_keyword.total, 3, "keyword 应命中 3 条活记录");
        let ids: Vec<u64> = by_keyword.items.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec![newer.id, middle.id, older.id],
            "应按 created_at 倒序"
        );
        assert_eq!(by_user.total, 1, "user_id 精确过滤");
        assert_eq!(by_user.items[0].id, middle.id);
        assert_eq!(by_status.total, 1, "status 精确过滤");
        assert_eq!(by_status.items[0].id, newer.id);
    }

    #[tokio::test]
    async fn soft_delete_batch_only_marks_alive_rows() {
        let db = test_db().await;
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
        let after = find_page(
            &db,
            &OperationLogFilter {
                keyword: Some(kw.clone()),
                user_id: None,
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[a.id, b.id, deleted.id]).await;

        assert_eq!(affected, 2, "已软删记录不应重复计入");
        assert_eq!(after.total, 0, "批量软删后全部不可见");
    }
}
