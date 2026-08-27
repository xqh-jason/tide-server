use sea_orm::DatabaseConnection;
use sea_orm::entity::prelude::*;

use crate::entity::sys_role;

pub async fn find_by_ids(
    db: &DatabaseConnection,
    ids: Vec<u64>,
) -> anyhow::Result<Vec<sys_role::Model>> {
    let roles = sys_role::Entity::find()
        .filter(sys_role::Column::Id.is_in(ids))
        .filter(sys_role::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, Database, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[tokio::test]
    async fn find_by_ids_excludes_deleted_roles() {
        let db = test_db().await;

        let live_role = sys_role::ActiveModel {
            role_name: Set("正常角色".to_string()),
            role_key: Set(unique("live_role")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let deleted_role = sys_role::ActiveModel {
            role_name: Set("已删除角色".to_string()),
            role_key: Set(unique("deleted_role")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut mark_deleted: sys_role::ActiveModel = deleted_role.clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
        mark_deleted.update(&db).await.unwrap();

        let found = find_by_ids(&db, vec![live_role.id, deleted_role.id]).await;

        sys_role::Entity::delete_by_id(live_role.id)
            .exec(&db)
            .await
            .unwrap();
        sys_role::Entity::delete_by_id(deleted_role.id)
            .exec(&db)
            .await
            .unwrap();

        let found = found.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, live_role.id);
    }
}
