use crate::entity::sys_user;
use crate::modules::user::repo as user_repo;

pub async fn get_by_username(
    db: &sea_orm::DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<sys_user::Model>> {
    let user = user_repo::find_by_username(db, username).await?;

    Ok(user)
}

/// 分页查询用户。`page_index` 为 0-based（由 handler 层从 PageQuery 转换）。
pub async fn page_users(
    db: &sea_orm::DatabaseConnection,
    keyword: Option<String>,
    status: Option<i8>,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<(u64, Vec<sys_user::Model>)> {
    user_repo::find_page(db, keyword, status, page_index, page_size).await
}
