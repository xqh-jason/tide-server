//! 定时任务执行日志数据访问（codegen 生成后裁剪：只读 + 软删）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryOrder};

use crate::entity::{sys_job_log, sys_job_log::Model};
use crate::modules::job_log::dto::JobLogFilter;

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
        .order_by_desc(sys_job_log::Column::CreatedAt);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 执行日志落库（job_runner 写入入口，无需审计盖章）。
pub async fn create_job_log(
    db: &impl ConnectionTrait,
    model: sys_job_log::ActiveModel,
) -> anyhow::Result<Model> {
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

/// 物理删除 created_at 早于 cutoff 的记录（定时清理任务用），返回受影响行数。
pub async fn delete_created_before(
    db: &impl ConnectionTrait,
    cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<u64> {
    let result = sys_job_log::Entity::delete_many()
        .filter(sys_job_log::Column::CreatedAt.lt(cutoff))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
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
