//! 操作日志业务（codegen 生成）。

use sea_orm::{ActiveValue::Set, DatabaseConnection};

use crate::entity::sys_operation_log;
use crate::modules::operation_log::dto::{CreateOperationLogReq, OperationLogFilter, OperationLogListReq, UpdateOperationLogReq};
use crate::modules::operation_log::repo as operation_log_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 分页查询。
pub async fn page_operation_logs(
    db: &DatabaseConnection,
    req: &OperationLogListReq,
) -> anyhow::Result<PageData<sys_operation_log::Model>> {
    operation_log_repo::find_page(
        db,
        &OperationLogFilter {
            user_id: req.user_id,
            status: req.status,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 创建：唯一字段查重（含软删占位）→ 构造 ActiveModel → 落库。
pub async fn create_operation_log(
    db: &DatabaseConnection,
    req: &CreateOperationLogReq,
) -> Result<sys_operation_log::Model, AppError> {

    let model = sys_operation_log::ActiveModel {
        user_id: Set(req.user_id),
        ip: Set(req.ip.clone().unwrap_or_default()),
        method: Set(req.method.clone().unwrap_or_default()),
        path: Set(req.path.clone()),
        status: Set(req.status.unwrap_or(0)),
        latency: Set(req.latency.unwrap_or(0)),
        agent: Set(req.agent.clone().unwrap_or_default()),
        body: Set(req.body.clone().unwrap_or_default()),
        resp: Set(req.resp.clone().unwrap_or_default()),
        error_message: Set(req.error_message.clone().unwrap_or_default()),
        ..Default::default()
    };
    let model = operation_log_repo::create_operation_log(db, model).await?;
    Ok(model)
}

/// 更新：判存在 → 唯一字段查重排除自身 → 全量覆盖。
pub async fn update_operation_log(
    db: &DatabaseConnection,
    req: &UpdateOperationLogReq,
) -> Result<sys_operation_log::Model, AppError> {
    let Some(_) = operation_log_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz(format!("操作日志不存在：{}", req.id)));
    };

    let model = sys_operation_log::ActiveModel {
        id: Set(req.id),
        user_id: Set(req.user_id),
        ip: Set(req.ip.clone()),
        method: Set(req.method.clone()),
        path: Set(req.path.clone()),
        status: Set(req.status),
        latency: Set(req.latency),
        agent: Set(req.agent.clone()),
        body: Set(req.body.clone()),
        resp: Set(req.resp.clone()),
        error_message: Set(req.error_message.clone()),
        ..Default::default()
    };
    let model = operation_log_repo::update_operation_log(db, model).await?;
    Ok(model)
}

/// 查询单个详情（排除软删除）。
pub async fn get_operation_log(db: &DatabaseConnection, id: u64) -> Result<sys_operation_log::Model, AppError> {
    let Some(model) = operation_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("操作日志不存在：{id}")));
    };
    Ok(model)
}

/// 删除：判存在后软删。
pub async fn delete_operation_log(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let Some(_) = operation_log_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("操作日志不存在：{id}")));
    };
    operation_log_repo::soft_delete_operation_log(db, id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_operation_log;
    use crate::modules::operation_log::dto::{CreateOperationLogReq, UpdateOperationLogReq};
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

    async fn seed(
        db: &DatabaseConnection,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_operation_log::Model {
        sys_operation_log::ActiveModel {
            user_id: Set(0),
            ip: Set(unique("ip")),
            method: Set(unique("method")),
            path: Set(unique("path")),
            status: Set(0),
            latency: Set(0),
            agent: Set(unique("agent")),
            body: Set(unique("body")),
            resp: Set(unique("resp")),
            error_message: Set(unique("error_message")),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn create_req() -> CreateOperationLogReq {
        CreateOperationLogReq {
            user_id: 0,
            ip: None,
            method: None,
            path: unique("path"),
            status: Some(0),
            latency: Some(0),
            agent: None,
            body: None,
            resp: None,
            error_message: None,
        }
    }

    fn update_req(id: u64) -> UpdateOperationLogReq {
        UpdateOperationLogReq {
            id,
            user_id: 0,
            ip: unique("ip"),
            method: unique("method"),
            path: unique("path"),
            status: 0,
            latency: 0,
            agent: unique("agent"),
            body: unique("body"),
            resp: unique("resp"),
            error_message: unique("error_message"),
        }
    }

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_operation_log::Entity::delete_many()
            .filter(sys_operation_log::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_rejects_duplicate__including_soft_deleted() {
        let db = test_db().await;
        let live = seed(&db, None).await;
        let deleted = seed(&db, Some(chrono::Utc::now().naive_utc())).await;

        let result_live = create_operation_log(&db, &create_req()).await;
        let result_deleted = create_operation_log(&db, &create_req()).await;

        cleanup(&db, &[live.id, deleted.id]).await;

        assert!(
            matches!(result_live, Err(AppError::Biz(_))),
            "正常占位应拒绝重复，实际：{result_live:?}"
        );
        assert!(
            matches!(result_deleted, Err(AppError::Biz(_))),
            "软删占位应拒绝重复，实际：{result_deleted:?}"
        );
    }

    #[tokio::test]
    async fn update_rejects_duplicate__excluding_self() {
        let db = test_db().await;
        let a = seed(&db, None).await;
        let b = seed(&db, None).await;

        let dup = update_operation_log(&db, &update_req(b.id)).await;
        let keep_self = update_operation_log(&db, &update_req(b.id)).await;

        cleanup(&db, &[a.id, b.id]).await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人唯一值应拒绝，实际：{dup:?}"
        );
        keep_self.expect("保留自身唯一值应更新成功");
    }

    #[tokio::test]
    async fn update_and_delete_return_biz_error_when_missing() {
        let db = test_db().await;

        let missing = update_operation_log(&db, &update_req(9_999_999_999)).await;
        let delete_missing = delete_operation_log(&db, 9_999_999_999).await;

        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "更新不存在应返回 Biz，实际：{missing:?}"
        );
        assert!(
            matches!(delete_missing, Err(AppError::Biz(_))),
            "删除不存在应返回 Biz，实际：{delete_missing:?}"
        );
    }
}
