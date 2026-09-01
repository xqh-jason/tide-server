//! 数据字典数据访问（codegen 生成）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection};

use crate::entity::{sys_dict, sys_dict::Model};
use crate::modules::dict::dto::DictFilter;

/// 查询单个有效记录（排除软删除）。
pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    let m = sys_dict::Entity::find()
        .filter(sys_dict::Column::Id.eq(id))
        .filter(sys_dict::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(m)
}

/// 分页 + 动态过滤查询（过滤条件由域定义驱动）。
pub async fn find_page(
    db: &DatabaseConnection,
    filter: &DictFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(v) = &filter.type_code {
        cond = cond.add(sys_dict::Column::TypeCode.eq(v));
    }
    if let Some(v) = &filter.label {
        cond = cond.add(sys_dict::Column::Label.like(format!("%{v}%")));
    }
    if let Some(v) = &filter.status {
        cond = cond.add(sys_dict::Column::Status.eq(*v));
    }

    let select = sys_dict::Entity::find()
        .filter(cond)
        .filter(sys_dict::Column::DeletedAt.is_null());
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建记录。
pub async fn create(db: &DatabaseConnection, model: sys_dict::ActiveModel) -> anyhow::Result<Model> {
    Ok(model.insert(db).await?)
}

/// 更新记录（主键必须已设置）。
pub async fn update(db: &DatabaseConnection, model: sys_dict::ActiveModel) -> anyhow::Result<Model> {
    Ok(model.update(db).await?)
}

/// 软删除：`deleted_at` 置为当前时间。
pub async fn soft_delete(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_dict::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

/// 查重辅助：type_code 唯一（含软删占位）。
pub async fn find_by_type_code_include_deleted(
    db: &DatabaseConnection,
    type_code: &str,
) -> anyhow::Result<Option<Model>> {
    let m = sys_dict::Entity::find()
        .filter(sys_dict::Column::TypeCode.eq(type_code))
        .one(db)
        .await?;
    Ok(m)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_dict;
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

    async fn seed(db: &DatabaseConnection, type_code: &str, deleted_at: Option<chrono::NaiveDateTime>) -> sys_dict::Model {
        sys_dict::ActiveModel {
            type_code: Set(type_code.to_string()),
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

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_dict::Entity::delete_many()
            .filter(sys_dict::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted() {
        let db = test_db().await;
        let live = seed(&db, &unique("live"), None).await;
        let deleted = seed(&db, &unique("deleted"), Some(chrono::Utc::now().naive_utc())).await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        cleanup(&db, &[live.id, deleted.id]).await;

        assert_eq!(found_live.as_ref().map(|m| m.id), Some(live.id));
        assert!(found_deleted.is_none(), "软删记录不应被 find_by_id 查到");
    }

    #[tokio::test]
    async fn soft_delete_sets_deleted_at() {
        let db = test_db().await;
        let m = seed(&db, &unique("soft"), None).await;

        let deleted = soft_delete(&db, m.id).await.unwrap();
        let after = find_by_id(&db, m.id).await.unwrap();

        cleanup(&db, &[m.id]).await;

        assert!(deleted);
        assert!(after.is_none(), "软删后不应再查到");
    }

    #[tokio::test]
    async fn find_page_filters_and_excludes_deleted() {
        let db = test_db().await;
        // 唯一索引含软删占位：正常与软删记录必须使用不同 type_code
        let kw_live = unique("page_live");
        let kw_deleted = unique("page_deleted");
        let live = seed(&db, &kw_live, None).await;
        let deleted = seed(&db, &kw_deleted, Some(chrono::Utc::now().naive_utc())).await;

        let data = find_page(
            &db,
            &crate::modules::dict::dto::DictFilter {
                type_code: Some(kw_live.clone()),
                label: None,
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();

        cleanup(&db, &[live.id, deleted.id]).await;

        assert_eq!(data.total, 1, "软删记录不应进入分页");
        assert_eq!(data.items[0].id, live.id);
    }
}
