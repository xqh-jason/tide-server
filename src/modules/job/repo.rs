//! 定时任务数据访问（codegen 生成后对齐规范：`&impl ConnectionTrait` + 审计盖章）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryOrder};

use crate::entity::{sys_job, sys_job::Model};
use crate::modules::job::dto::JobFilter;

/// 查询单个有效记录（排除软删除）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let m = sys_job::Entity::find()
        .filter(sys_job::Column::Id.eq(id))
        .filter(sys_job::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(m)
}

/// 分页 + 动态过滤查询（keyword 对 job_name 模糊，status 精确，
/// created_by/updated_by/时间范围审计过滤），排除软删，created_at 倒序。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &JobFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();

    if let Some(v) = &filter.job_name {
        cond = cond.add(sys_job::Column::JobName.like(format!("%{v}%")));
    }
    if let Some(v) = &filter.status {
        cond = cond.add(sys_job::Column::Status.eq(*v));
    }
    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_job::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_job::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_job::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_job::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_job::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_job::Column::UpdatedAt.lte(v));
    }

    let select = sys_job::Entity::find()
        .filter(cond)
        .filter(sys_job::Column::DeletedAt.is_null());
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建记录。`actor_id` 为操作人，审计字段由 repo 统一盖章（create 双写同源）。
pub async fn create_job(
    db: &impl ConnectionTrait,
    model: sys_job::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Model> {
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    Ok(model.insert(db).await?)
}

/// 更新记录（主键必须已设置）。只刷新 updated_by，created_by 保持 NotSet 不被覆盖。
pub async fn update_job(
    db: &impl ConnectionTrait,
    model: sys_job::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Model> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    Ok(model.update(db).await?)
}

/// 软删除：`deleted_at` 置为当前时间。
pub async fn soft_delete_job(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_job::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

/// 启用 / 停用：status 翻转 + 刷新 updated_by。
pub async fn update_job_status(
    db: &impl ConnectionTrait,
    id: u64,
    status: i8,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_job::ActiveModel = model.into();
    model.status = Set(status);
    model.updated_by = Set(actor_id);
    model.update(db).await?;
    Ok(true)
}

/// 查重辅助：job_name 唯一（含软删占位）。
pub async fn find_by_job_name_include_deleted(
    db: &impl ConnectionTrait,
    job_name: &str,
) -> anyhow::Result<Option<Model>> {
    let m = sys_job::Entity::find()
        .filter(sys_job::Column::JobName.eq(job_name))
        .one(db)
        .await?;
    Ok(m)
}

/// 启动装载：查全部启用且未删除的任务（调度器初始化用），id 升序保证装载顺序稳定。
pub async fn find_active_jobs(db: &impl ConnectionTrait) -> anyhow::Result<Vec<Model>> {
    let models = sys_job::Entity::find()
        .filter(sys_job::Column::Status.eq(1))
        .filter(sys_job::Column::DeletedAt.is_null())
        .order_by_asc(sys_job::Column::Id)
        .all(db)
        .await?;
    Ok(models)
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

    async fn seed_job(
        db: &impl ConnectionTrait,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_job::Model {
        sys_job::ActiveModel {
            // uk_job_name 唯一（含软删行占位），每行用不同名称
            job_name: Set(unique("repo_job")),
            cron_expr: Set("0 0 3 * * *".to_string()),
            handler_name: Set("cleanup_login_logs".to_string()),
            status: Set(status),
            remark: Set(String::new()),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_by_job_name_include_deleted_matches_soft_deleted() {
        let db = test_txn().await;
        let live = seed_job(&db, 1, None).await;
        let deleted = seed_job(&db, 1, Some(chrono::Local::now().naive_local())).await;

        let hit_live = find_by_job_name_include_deleted(&db, &live.job_name).await;
        let hit_deleted = find_by_job_name_include_deleted(&db, &deleted.job_name).await;

        assert_eq!(hit_live.unwrap().unwrap().id, live.id, "活记录应命中");
        assert_eq!(
            hit_deleted.unwrap().unwrap().id,
            deleted.id,
            "软删占位也应命中（查重语义）"
        );
    }

    #[tokio::test]
    async fn find_active_jobs_returns_only_enabled_undeleted() {
        let db = test_txn().await;
        let enabled = seed_job(&db, 1, None).await;
        let _disabled = seed_job(&db, 0, None).await;
        let _deleted = seed_job(&db, 1, Some(chrono::Local::now().naive_local())).await;

        let jobs = find_active_jobs(&db).await.unwrap();

        // 测试事务能看见真库已提交数据（如种子示例任务），断言按 id 归属而非精确条数
        let ids: Vec<u64> = jobs.iter().map(|j| j.id).collect();
        assert!(ids.contains(&enabled.id), "启用且未删的任务应被装载");
        assert!(
            !ids.contains(&_disabled.id),
            "停用任务不应被装载，实际：{ids:?}"
        );
        assert!(
            !ids.contains(&_deleted.id),
            "软删任务不应被装载，实际：{ids:?}"
        );
    }

    #[tokio::test]
    async fn soft_delete_job_sets_deleted_at() {
        let db = test_txn().await;
        let m = seed_job(&db, 1, None).await;

        let deleted = soft_delete_job(&db, m.id).await.unwrap();
        let after = find_by_id(&db, m.id).await.unwrap();

        assert!(deleted);
        assert!(after.is_none(), "软删后不应再查到");
    }

    #[tokio::test]
    async fn find_page_filters_by_keyword_status_and_audit_columns() {
        let db = test_txn().await;
        let kw = unique("page_job");
        let base = chrono::Local::now().naive_local();

        let a = sys_job::ActiveModel {
            job_name: Set(format!("{kw}a")),
            cron_expr: Set("0 0 3 * * *".to_string()),
            handler_name: Set("cleanup_login_logs".to_string()),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let b = sys_job::ActiveModel {
            job_name: Set(format!("{kw}b")),
            cron_expr: Set("0 0 4 * * *".to_string()),
            handler_name: Set("cleanup_job_logs".to_string()),
            status: Set(0),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        // update_many 盖不同的审计人与时间：避免为测试改 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_job::Entity::update_many()
                .filter(sys_job::Column::Id.eq(row))
                .col_expr(sys_job::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_job::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_job::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_job::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let by_creator = find_page(
            &db,
            &JobFilter {
                job_name: Some(kw.clone()),
                created_by: Some(7),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_creator.total, 1, "created_by=7 应只命中 a");

        let by_status = find_page(
            &db,
            &JobFilter {
                job_name: Some(kw.clone()),
                status: Some(0),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_status.total, 1, "status=0 应只命中 b");

        let by_time = find_page(
            &db,
            &JobFilter {
                job_name: Some(kw.clone()),
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
}
