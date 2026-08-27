use sea_orm::ActiveValue::Set;
use sea_orm::DatabaseConnection;

use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::modules::role::service as role_service;
use crate::modules::user::dto::{CreateUserReq, UserInfoResp, UserResp};
use crate::modules::user::repo as user_repo;
use crate::utils::crypt;
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
    db: &DatabaseConnection,
    roles: &[String],
) -> anyhow::Result<Vec<String>> {
    if roles.iter().any(|r| r == "super") {
        return Ok(vec!["super".to_string()]);
    }
    // TODO(W3)：sys_role_menu → sys_menu(menu_type=3) 拍平 permission 码，与后端授权点同源
    let _ = db;
    Ok(vec![])
}

/// 创建用户
pub async fn create_user(
    db: &DatabaseConnection,
    req: CreateUserReq,
) -> Result<UserResp, AppError> {
    // username唯一性检测
    if user_repo::find_by_username_include_deleted(db, &req.username)
        .await?
        .is_some()
    {
        return Err(AppError::Biz("用户名已存在".into()));
    }

    // role_ids 检查
    if req.role_ids.len() > 0 {
        // 检查角色是否重复
        let mut unique_role_ids = req.role_ids.clone();
        unique_role_ids.sort();
        unique_role_ids.dedup();
        if req.role_ids.len() != unique_role_ids.len() {
            return Err(AppError::Biz("角色ID重复".into()));
        }

        let mut role_err_msgs = Vec::new();

        let roles = role_service::find_by_ids(db, req.role_ids.clone()).await?;
        // 检查角色是否都存在
        let found_role_ids = roles.iter().map(|r| r.id).collect::<Vec<_>>();
        let messing_role_ids = req
            .role_ids
            .iter()
            .filter(|id| !found_role_ids.contains(id))
            .collect::<Vec<_>>();
        // 检测全部再一次返回错误信息
        if !messing_role_ids.is_empty() {
            role_err_msgs.push(format!(
                "角色不存在：{}",
                messing_role_ids
                    .iter()
                    .map(|id| format!("{}", id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        for role in roles {
            if role.status != 1 || role.deleted_at.is_some() {
                role_err_msgs.push(format!("角色 {} 不存在或已禁用", role.role_name));
            }
        }
        if !role_err_msgs.is_empty() {
            return Err(AppError::Biz(format!("{}", role_err_msgs.join(", "))));
        }
    }

    let model = user_repo::create_user_with_roles(
        db,
        sys_user::ActiveModel {
            username: Set(req.username),
            password: Set(crypt::hash_password(&req.password)?),
            nickname: Set(req.nickname),
            phone: Set(req.phone.unwrap_or_default()),
            email: Set(req.email.unwrap_or_default()),
            status: Set(req.status.unwrap_or(1)),
            ..Default::default()
        },
        req.role_ids,
    )
    .await?;

    Ok(UserResp::from(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_role, sys_user_role};
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_name(prefix: &str) -> String {
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

    fn request(username: String, role_ids: Vec<u64>) -> CreateUserReq {
        CreateUserReq {
            username,
            password: "pass123".to_string(),
            nickname: "创建用户测试".to_string(),
            phone: Some("13800138000".to_string()),
            email: Some("created@example.com".to_string()),
            status: Some(1),
            role_ids,
        }
    }

    #[tokio::test]
    async fn create_user_links_enabled_roles_and_stores_hashed_password() {
        let db = test_db().await;
        let username = unique_name("create_user");
        let role_key = unique_name("created");

        let role = sys_role::ActiveModel {
            role_name: Set("可分配角色".to_string()),
            role_key: Set(role_key),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let created = create_user(&db, request(username.clone(), vec![role.id]))
            .await
            .unwrap();

        assert_eq!(created.username, username);
        assert_eq!(created.nickname, "创建用户测试");

        let saved = user_repo::find_by_id(&db, created.id)
            .await
            .unwrap()
            .expect("用户应已保存");
        assert!(saved.password.starts_with("$argon2id$"));
        assert_eq!(saved.email, "created@example.com");

        let links = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].role_id, role.id);

        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(created.id)
            .exec(&db)
            .await
            .unwrap();
        sys_role::Entity::delete_by_id(role.id)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_user_rejects_unknown_role_ids_without_saving_user() {
        let db = test_db().await;
        let username = unique_name("create_user_invalid_role");

        let result = create_user(&db, request(username.clone(), vec![u64::MAX])).await;

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("角色")),
            "应返回业务错误: {result:?}"
        );
        assert!(
            user_repo::find_by_username(&db, &username)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn get_user_info_rejects_deleted_user() {
        let db = test_db().await;
        let username = unique_name("deleted_info_user");

        let user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("已删除信息用户".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut mark_deleted: sys_user::ActiveModel = user.clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
        mark_deleted.update(&db).await.unwrap();

        let auth = AuthUser {
            user_id: user.id,
            username: username.clone(),
            roles: vec![],
        };
        let result = get_user_info(&db, &auth).await;

        // 先物理清理，避免断言失败时残留测试数据。
        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("用户不存在")),
            "已删除用户不应获取用户信息: {result:?}"
        );
    }

    #[tokio::test]
    async fn create_user_rejects_username_matching_soft_deleted_user() {
        let db = test_db().await;
        let username = unique_name("deleted_unique_user");

        let old_user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("旧同名用户".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut mark_deleted: sys_user::ActiveModel = old_user.clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
        mark_deleted.update(&db).await.unwrap();

        let result = create_user(&db, request(username.clone(), vec![])).await;

        // 唯一约束来自原同名记录，因此这里必须物理清理旧数据。
        sys_user::Entity::delete_by_id(old_user.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("用户名已存在")),
            "软删除用户名也不应允许重复创建: {result:?}"
        );
    }

    #[tokio::test]
    async fn create_user_rejects_duplicate_role_ids() {
        let db = test_db().await;
        let username = unique_name("duplicate_role_user");
        let role_key = unique_name("duplicate_role");

        let role = sys_role::ActiveModel {
            role_name: Set("重复角色测试".to_string()),
            role_key: Set(role_key),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        // 乱序传入两个不同 ID 的场景由另一个用例覆盖；这里验证同一个 ID 重复。
        let result = create_user(&db, request(username.clone(), vec![role.id, role.id])).await;

        assert!(
            user_repo::find_by_username(&db, &username)
                .await
                .unwrap()
                .is_none(),
            "校验失败时不应创建用户"
        );

        sys_role::Entity::delete_by_id(role.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("重复")),
            "重复角色 ID 应返回业务错误: {result:?}"
        );
    }

    #[tokio::test]
    async fn create_user_accepts_unsorted_distinct_role_ids() {
        let db = test_db().await;
        let username = unique_name("unsorted_roles_user");

        let mut roles = Vec::new();
        for order in [2, 0, 1] {
            roles.push(
                sys_role::ActiveModel {
                    role_name: Set(format!("乱序角色 {order}")),
                    role_key: Set(unique_name("unsorted_role")),
                    sort: Set(order as i32),
                    status: Set(1),
                    remark: Set(String::new()),
                    ..Default::default()
                }
                .insert(&db)
                .await
                .unwrap(),
            );
        }

        // MySQL 自增 ID 通常按插入顺序生成；这里反向传入，验证“未排序”而不只是“不同”。
        let role_ids = roles.iter().rev().map(|role| role.id).collect();
        let result = create_user(&db, request(username.clone(), role_ids)).await;

        let links = sys_user_role::Entity::find()
            .filter(
                sys_user_role::Column::UserId.eq(result.as_ref().map_or(u64::MAX, |user| user.id)),
            )
            .all(&db)
            .await
            .unwrap();

        if let Ok(created) = &result {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(created.id))
                .exec(&db)
                .await
                .unwrap();
            sys_user::Entity::delete_by_id(created.id)
                .exec(&db)
                .await
                .unwrap();
        }
        for role in roles {
            sys_role::Entity::delete_by_id(role.id)
                .exec(&db)
                .await
                .unwrap();
        }

        let created = result.expect("不同且未排序的角色 ID 应创建成功");
        assert_eq!(created.username, username);
        assert_eq!(links.len(), 3);
    }
}
