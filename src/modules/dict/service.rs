//! 数据字典业务（codegen 生成）。

use sea_orm::{ActiveValue::Set, DatabaseConnection};

use crate::entity::sys_dict;
use crate::modules::dict::dto::{CreateDictReq, DictFilter, DictListReq, UpdateDictReq};
use crate::modules::dict::repo as dict_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 分页查询。
pub async fn page_dicts(
    db: &DatabaseConnection,
    req: &DictListReq,
) -> anyhow::Result<PageData<sys_dict::Model>> {
    dict_repo::find_page(
        db,
        &DictFilter {
            type_code: req.type_code.clone(),
            label: req.label.clone(),
            status: req.status,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 创建：唯一字段查重（含软删占位）→ 构造 ActiveModel → 落库。
pub async fn create_dict(
    db: &DatabaseConnection,
    req: &CreateDictReq,
) -> Result<sys_dict::Model, AppError> {
    if let Some(existing) = dict_repo::find_by_type_code_include_deleted(db, &req.type_code).await? {
        return Err(AppError::Biz(format!("字典类型编码已存在：{}", existing.type_code)));
    }

    let model = sys_dict::ActiveModel {
        type_code: Set(req.type_code.clone()),
        label: Set(req.label.clone()),
        value: Set(req.value.clone()),
        sort: Set(req.sort.unwrap_or(0)),
        status: Set(req.status.unwrap_or(1)),
        remark: Set(req.remark.clone().unwrap_or_default()),
        ..Default::default()
    };
    let model = dict_repo::create(db, model).await?;
    Ok(model)
}

/// 更新：判存在 → 唯一字段查重排除自身 → 全量覆盖。
pub async fn update_dict(
    db: &DatabaseConnection,
    req: &UpdateDictReq,
) -> Result<sys_dict::Model, AppError> {
    let Some(_) = dict_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz(format!("数据字典不存在：{}", req.id)));
    };
    if let Some(existing) = dict_repo::find_by_type_code_include_deleted(db, &req.type_code).await? {
        if existing.id != req.id {
            return Err(AppError::Biz(format!("字典类型编码已存在：{}", existing.type_code)));
        }
    }

    let model = sys_dict::ActiveModel {
        id: Set(req.id),
        type_code: Set(req.type_code.clone()),
        label: Set(req.label.clone()),
        value: Set(req.value.clone()),
        sort: Set(req.sort),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };
    let model = dict_repo::update(db, model).await?;
    Ok(model)
}

/// 查询单个详情（排除软删除）。
pub async fn get_dict(db: &DatabaseConnection, id: u64) -> Result<sys_dict::Model, AppError> {
    let Some(model) = dict_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("数据字典不存在：{id}")));
    };
    Ok(model)
}

/// 删除：判存在后软删。
pub async fn delete_dict(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let Some(_) = dict_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("数据字典不存在：{id}")));
    };
    dict_repo::soft_delete(db, id).await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_dict;
    use crate::modules::dict::dto::{CreateDictReq, UpdateDictReq};
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

    async fn seed(db: &DatabaseConnection, deleted_at: Option<chrono::NaiveDateTime>) -> sys_dict::Model {
        sys_dict::ActiveModel {
            type_code: Set(unique("type_code")),
            label: Set(unique("label")),
            value: Set(unique("value")),
            sort: Set(0),
            status: Set(1),
            remark: Set(unique("remark")),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn create_req(type_code: String) -> CreateDictReq {
        CreateDictReq {
            type_code,
            label: unique("label"),
            value: unique("value"),
            sort: Some(0),
            status: Some(1),
            remark: None,
        }
    }

    fn update_req(id: u64, type_code: String) -> UpdateDictReq {
        UpdateDictReq {
            id,
            type_code,
            label: unique("label"),
            value: unique("value"),
            sort: 0,
            status: 0,
            remark: unique("remark"),
        }
    }

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_dict::Entity::delete_many()
            .filter(sys_dict::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_rejects_duplicate_type_code_including_soft_deleted() {
        let db = test_db().await;
        let live = seed(&db, None).await;
        let deleted = seed(&db, Some(chrono::Utc::now().naive_utc())).await;

        let result_live = create_dict(&db, &create_req(live.type_code.clone())).await;
        let result_deleted = create_dict(&db, &create_req(deleted.type_code.clone())).await;

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
    async fn update_rejects_duplicate_type_code_excluding_self() {
        let db = test_db().await;
        let a = seed(&db, None).await;
        let b = seed(&db, None).await;

        let dup = update_dict(&db, &update_req(b.id, a.type_code.clone())).await;
        let keep_self = update_dict(&db, &update_req(b.id, b.type_code.clone())).await;

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

        let missing = update_dict(&db, &update_req(9_999_999_999, unique("missing"))).await;
        let delete_missing = delete_dict(&db, 9_999_999_999).await;

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
