//! 权限业务规则层。

use crate::modules::permission::SUPER_ROLE_KEY;
use crate::modules::permission::repo as permission_repo;
use crate::modules::user::repo as user_repo;
use crate::utils::error::AppError;
use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection};

/// 判断用户是否拥有指定操作权限。
///
/// `super` 角色作为项目当前约定拥有全部权限。
pub async fn has_permission(
    db: &impl ConnectionTrait,
    user_id: u64,
    permission_code: &str,
) -> Result<bool, AppError> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;
    let role_keys = roles
        .into_iter()
        .map(|role| role.role_key)
        .collect::<Vec<_>>();
    if role_keys.contains(&SUPER_ROLE_KEY.to_string()) {
        return Ok(true);
    }

    let permission_codes = permission_repo::find_permission_codes_by_user_id(db, user_id).await?;
    Ok(permission_codes.contains(&permission_code.to_string()))
}
