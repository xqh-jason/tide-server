use crate::modules::role::repo as role_repo;
use sea_orm::DatabaseConnection;

use crate::entity::sys_role;

pub async fn find_by_ids(
    db: &DatabaseConnection,
    ids: Vec<u64>,
) -> anyhow::Result<Vec<sys_role::Model>> {
    let roles = role_repo::find_by_ids(db, ids).await?;
    Ok(roles)
}
