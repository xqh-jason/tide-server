use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::modules::user::repo as user_repo;
use crate::modules::user::dto::UserInfoResp;
use crate::utils::error::AppError;

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

/// 当前登录用户完整信息（契约 §3.2 的 `/user/info`）。
pub async fn get_user_info(
    db: &sea_orm::DatabaseConnection,
    auth: &AuthUser,
) -> Result<UserInfoResp, AppError> {
    let user = user_repo::find_by_id(db, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Biz("用户不存在".into()))?;
    Ok(UserInfoResp::from_model(user, auth))
}

/// 权限码数组（契约 §3.2 的 `/user/access-codes`）：
/// 超管返回 `['super']`；普通用户按角色查按钮权限码（W3 完善）。
pub async fn get_access_codes(
    db: &sea_orm::DatabaseConnection,
    roles: &[String],
) -> anyhow::Result<Vec<String>> {
    if roles.iter().any(|r| r == "super") {
        return Ok(vec!["super".to_string()]);
    }
    // TODO(W3)：sys_role_menu → sys_menu(menu_type=3) 拍平 permission 码，与后端授权点同源
    let _ = db;
    Ok(vec![])
}
