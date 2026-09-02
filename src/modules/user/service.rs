use sea_orm::ActiveValue::Set;
use sea_orm::DatabaseConnection;

use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::modules::permission::repo as permission_repo;
use crate::modules::permission::{
    ADMIN_USERNAME, SUPER_ROLE_KEY, SYSTEM_USER_CREATE, SYSTEM_USER_UPDATE,
    service as permission_service,
};
use crate::modules::role::service as role_service;
use crate::modules::user::dto::{
    CreateUserReq, UpdateUserReq, UserFilter, UserInfoResp, UserListReq, UserResp,
};
use crate::modules::user::repo as user_repo;
use crate::utils::PageData;
use crate::utils::crypt;
use crate::utils::error::AppError;

pub async fn get_by_username(
    db: &sea_orm::DatabaseConnection,
    username: &str,
) -> Result<Option<sys_user::Model>, AppError> {
    let user = user_repo::find_by_username(db, username).await?;

    Ok(user)
}

/// 分页查询用户。`page_index` 为 0-based（由 handler 层从 PageQuery 转换）。
pub async fn page_users(
    db: &sea_orm::DatabaseConnection,
    req: &UserListReq,
) -> Result<PageData<sys_user::Model>, AppError> {
    let model = user_repo::find_page(
        db,
        &UserFilter {
            keyword: req.keyword.clone(),
            status: req.status,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(model)
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
    user_id: u64,
) -> Result<Vec<String>, AppError> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;
    if roles.iter().any(|role| role.role_key == SUPER_ROLE_KEY) {
        return Ok(vec![SUPER_ROLE_KEY.to_string()]);
    }

    let permissions = permission_repo::find_permission_codes_by_user_id(db, user_id).await?;
    Ok(permissions)
}

/// 创建用户（发起人 `actor_id` 需拥有 `system:user:create` 权限）。
pub async fn create_user(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateUserReq,
) -> Result<UserResp, AppError> {
    if !permission_service::has_permission(db, actor_id, SYSTEM_USER_CREATE).await? {
        return Err(AppError::Biz("用户没有创建用户权限".into()));
    }

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

    let model = user_repo::create_user_with_links(
        db,
        sys_user::ActiveModel {
            username: Set(req.username),
            password: Set(crypt::hash_password(&req.password)?),
            emp_no: Set(req.emp_no),
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

pub async fn get_user(db: &DatabaseConnection, user_id: u64) -> Result<UserResp, AppError> {
    let user = user_repo::find_by_id(db, user_id).await?;

    if user.is_none() {
        return Err(AppError::Biz("用户不存在".into()));
    }
    Ok(UserResp::from(user.unwrap()))
}

/// 更新用户（发起人 `actor_id` 需拥有 `system:user:update` 权限）。
///
/// 内置超管 admin（`ADMIN_USERNAME`）不允许被编辑：其密码由启动 seed 统一重置，
/// 编辑入口拦截，防止账号被禁用或角色被改导致系统失守。
pub async fn update_user_with_links(
    db: &DatabaseConnection,
    actor_id: u64,
    req: UpdateUserReq,
) -> Result<UserResp, AppError> {
    if !permission_service::has_permission(db, actor_id, SYSTEM_USER_UPDATE).await? {
        return Err(AppError::Biz("用户没有更新用户权限".into()));
    }

    let Some(user) = user_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz("用户不存在".into()));
    };

    // 内置超管不允许编辑。
    if user.username == ADMIN_USERNAME {
        return Err(AppError::Biz("系统内置管理员不允许编辑".into()));
    }
    // 普通用户也不允许改名顶替内置超管。
    if req.username == ADMIN_USERNAME {
        return Err(AppError::Biz("用户名不能与系统内置管理员相同".into()));
    }

    // username 唯一性（含软删占位，排除自身）。
    if let Some(existing) = user_repo::find_by_username_include_deleted(db, &req.username).await? {
        if existing.id != req.id {
            return Err(AppError::Biz("用户名已存在".into()));
        }
    }

    // role_ids 检查（与 create 一致）：去重、存在性、启用状态。
    if req.role_ids.len() > 0 {
        let mut unique_role_ids = req.role_ids.clone();
        unique_role_ids.sort();
        unique_role_ids.dedup();
        if req.role_ids.len() != unique_role_ids.len() {
            return Err(AppError::Biz("角色ID重复".into()));
        }

        let mut role_err_msgs = Vec::new();
        let roles = role_service::find_by_ids(db, req.role_ids.clone()).await?;
        let found_role_ids = roles.iter().map(|r| r.id).collect::<Vec<_>>();
        let messing_role_ids = req
            .role_ids
            .iter()
            .filter(|id| !found_role_ids.contains(id))
            .collect::<Vec<_>>();
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
            return Err(AppError::Biz(role_err_msgs.join(", ")));
        }
    }

    // 密码为空代表不更新密码（沿用库中原有密文）。
    let password = if req.password.is_empty() {
        user.password
    } else {
        crypt::hash_password(&req.password)?
    };

    let model = user_repo::update_user_with_links(
        db,
        sys_user::ActiveModel {
            id: Set(req.id),
            username: Set(req.username),
            password: Set(password),
            emp_no: Set(req.emp_no),
            nickname: Set(req.nickname),
            phone: Set(req.phone),
            email: Set(req.email),
            status: Set(req.status),
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
    use crate::entity::{sys_menu, sys_role, sys_role_menu, sys_user, sys_user_role};
    use crate::modules::permission::{
        ADMIN_USERNAME, SUPER_ROLE_KEY, SYSTEM_USER_CREATE, SYSTEM_USER_UPDATE,
    };
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
            emp_no: "T1001".to_string(),
            nickname: "创建用户测试".to_string(),
            phone: Some("13800138000".to_string()),
            email: Some("created@example.com".to_string()),
            status: Some(1),
            role_ids,
        }
    }

    fn update_request(id: u64, username: String, role_ids: Vec<u64>) -> UpdateUserReq {
        UpdateUserReq {
            id,
            username,
            password: String::new(), // 空串表示不更新密码
            emp_no: "T2001".to_string(),
            nickname: "更新用户测试".to_string(),
            phone: "13900139000".to_string(),
            email: "updated@example.com".to_string(),
            status: 1,
            role_ids,
        }
    }

    /// 直接插入一个目标用户，供 update 测试使用（不依赖 create 权限）。
    async fn seed_target_user(db: &DatabaseConnection, username: String) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(username),
            password: Set("x".to_string()),
            nickname: Set("更新目标用户".to_string()),
            emp_no: Set("T1001".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 返回内置超管 admin（不存在时创建，返回 owned 标记以便测试后清理）。
    async fn load_or_create_admin(db: &DatabaseConnection) -> (sys_user::Model, bool) {
        if let Some(user) = user_repo::find_by_username_include_deleted(db, ADMIN_USERNAME)
            .await
            .unwrap()
        {
            return (user, false);
        }
        let user = sys_user::ActiveModel {
            username: Set(ADMIN_USERNAME.to_string()),
            password: Set("x".to_string()),
            nickname: Set("内置管理员(测试)".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        (user, true)
    }

    /// 已存在的 `super` 是全局唯一角色，测试只能复用；只有新创建时才物理清理。
    struct SuperRoleFixture {
        role: sys_role::Model,
        owned_by_test: bool,
    }

    /// 授权操作者（actor）：create_user 的发起人。
    async fn seed_actor(
        db: &DatabaseConnection,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique_name("actor")),
            password: Set("x".to_string()),
            nickname: Set("授权操作者".to_string()),
            status: Set(1),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn load_super_role(db: &DatabaseConnection) -> SuperRoleFixture {
        let existing = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq(SUPER_ROLE_KEY))
            .one(db)
            .await
            .unwrap();

        if let Some(role) = existing {
            return SuperRoleFixture {
                role,
                owned_by_test: false,
            };
        }

        let role = sys_role::ActiveModel {
            role_name: Set("超级管理员".to_string()),
            role_key: Set(SUPER_ROLE_KEY.to_string()),
            sort: Set(0),
            status: Set(1),
            remark: Set("授权测试创建".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        SuperRoleFixture {
            role,
            owned_by_test: true,
        }
    }

    async fn seed_role(
        db: &DatabaseConnection,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique_name("actor_role")),
            role_key: Set(unique_name("actor_role_key")),
            sort: Set(0),
            status: Set(status),
            remark: Set(String::new()),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_button(db: &DatabaseConnection, permission: &str) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(0),
            title: Set(unique_name("actor_menu")),
            name: Set(unique_name("ActorMenu")),
            menu_type: Set(3),
            permission: Set(permission.to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn bind_user_role(db: &DatabaseConnection, user_id: u64, role_id: u64) {
        sys_user_role::ActiveModel {
            user_id: Set(user_id),
            role_id: Set(role_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn bind_role_menu(db: &DatabaseConnection, role_id: u64, menu_id: u64) {
        sys_role_menu::ActiveModel {
            role_id: Set(role_id),
            menu_id: Set(menu_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// 清理 actor 及其绑定关系；super 角色共享时不物理删除。
    async fn cleanup_actor(
        db: &DatabaseConnection,
        user: &sys_user::Model,
        roles: &[sys_role::Model],
        menus: &[sys_menu::Model],
        super_fixture: Option<SuperRoleFixture>,
    ) {
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(user.id))
            .exec(db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(user.id)
            .exec(db)
            .await
            .unwrap();

        for role in roles {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(role.id))
                .exec(db)
                .await
                .unwrap();
            sys_role::Entity::delete_by_id(role.id)
                .exec(db)
                .await
                .unwrap();
        }

        for menu in menus {
            sys_menu::Entity::delete_by_id(menu.id)
                .exec(db)
                .await
                .unwrap();
        }

        if let Some(fixture) = super_fixture.filter(|item| item.owned_by_test) {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(fixture.role.id))
                .exec(db)
                .await
                .unwrap();
            sys_role::Entity::delete_by_id(fixture.role.id)
                .exec(db)
                .await
                .unwrap();
        }
    }

    /// create_user 授权测试。覆盖 W3 计划步骤 5 的 service 层场景；
    /// “无登录请求”与“发起人已软删除”由 AuthRequired 中间件保证，不在本层重复。
    #[tokio::test]
    async fn create_user_denies_actor_without_permission_and_saves_nothing() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let role = seed_role(&db, 1, None).await;
        bind_user_role(&db, actor.id, role.id).await;

        let target = unique_name("no_perm_target");
        let result = create_user(&db, actor.id, request(target.clone(), vec![])).await;

        cleanup_actor(&db, &actor, &[role], &[], None).await;

        assert!(
            result.is_err(),
            "无 system:user:create 权限应被拒绝: {result:?}"
        );
        assert!(
            user_repo::find_by_username_include_deleted(&db, &target)
                .await
                .unwrap()
                .is_none(),
            "无权限时不应创建目标用户"
        );
    }

    #[tokio::test]
    async fn create_user_allows_actor_with_button_permission() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let role = seed_role(&db, 1, None).await;
        let menu = seed_button(&db, SYSTEM_USER_CREATE).await;
        bind_user_role(&db, actor.id, role.id).await;
        bind_role_menu(&db, role.id, menu.id).await;

        let target = unique_name("perm_ok_target");
        let result = create_user(&db, actor.id, request(target.clone(), vec![])).await;

        let created = result.expect("拥有 system:user:create 的普通用户应创建成功");
        let saved = user_repo::find_by_id(&db, created.id).await.unwrap();
        cleanup_actor(&db, &actor, &[role], &[menu], None).await;
        if let Some(saved) = saved.as_ref() {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(saved.id))
                .exec(&db)
                .await
                .unwrap();
            sys_user::Entity::delete_by_id(saved.id)
                .exec(&db)
                .await
                .unwrap();
        }

        assert_eq!(
            saved.as_ref().map(|user| user.username.as_str()),
            Some(target.as_str())
        );
    }

    #[tokio::test]
    async fn create_user_allows_super_actor() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;

        let target = unique_name("super_target");
        let result = create_user(&db, actor.id, request(target.clone(), vec![])).await;

        let created = result.expect("有效 super 角色应创建成功");
        let saved = user_repo::find_by_id(&db, created.id).await.unwrap();
        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;
        if let Some(saved) = saved.as_ref() {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(saved.id))
                .exec(&db)
                .await
                .unwrap();
            sys_user::Entity::delete_by_id(saved.id)
                .exec(&db)
                .await
                .unwrap();
        }

        assert_eq!(
            saved.as_ref().map(|user| user.username.as_str()),
            Some(target.as_str())
        );
    }

    #[tokio::test]
    async fn create_user_denies_actor_whose_roles_are_disabled_or_deleted() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let disabled_role = seed_role(&db, 0, None).await;
        let deleted_role = seed_role(&db, 1, Some(chrono::Utc::now().naive_utc())).await;
        bind_user_role(&db, actor.id, disabled_role.id).await;
        bind_user_role(&db, actor.id, deleted_role.id).await;

        // JWT 里可能还保留旧角色（如 super），但授权只看数据库实时状态。
        let target = unique_name("stale_role_target");
        let result = create_user(&db, actor.id, request(target.clone(), vec![])).await;

        cleanup_actor(&db, &actor, &[disabled_role, deleted_role], &[], None).await;

        assert!(
            result.is_err(),
            "库中角色已禁用/删除时即使 JWT 保留旧角色也应拒绝: {result:?}"
        );
        assert!(
            user_repo::find_by_username_include_deleted(&db, &target)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn create_user_links_enabled_roles_and_stores_hashed_password() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;
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

        let created = create_user(&db, actor.id, request(username.clone(), vec![role.id]))
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
        assert_eq!(saved.emp_no, "T1001", "创建用户时应写入工号");

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
        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;
    }

    #[tokio::test]
    async fn create_user_rejects_unknown_role_ids_without_saving_user() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;
        let username = unique_name("create_user_invalid_role");

        let result = create_user(&db, actor.id, request(username.clone(), vec![u64::MAX])).await;

        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;

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
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;
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

        let result = create_user(&db, actor.id, request(username.clone(), vec![])).await;

        // 唯一约束来自原同名记录，因此这里必须物理清理旧数据。
        sys_user::Entity::delete_by_id(old_user.id)
            .exec(&db)
            .await
            .unwrap();
        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("用户名已存在")),
            "软删除用户名也不应允许重复创建: {result:?}"
        );
    }

    #[tokio::test]
    async fn create_user_rejects_duplicate_role_ids() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;
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
        let result = create_user(
            &db,
            actor.id,
            request(username.clone(), vec![role.id, role.id]),
        )
        .await;

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
        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("重复")),
            "重复角色 ID 应返回业务错误: {result:?}"
        );
    }

    #[tokio::test]
    async fn create_user_accepts_unsorted_distinct_role_ids() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;
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
        let result = create_user(&db, actor.id, request(username.clone(), role_ids)).await;

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
        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;

        let created = result.expect("不同且未排序的角色 ID 应创建成功");
        assert_eq!(created.username, username);
        assert_eq!(links.len(), 3);
    }

    /// access-codes 契约（W3 步骤 7）：super 返回超管标识，普通用户返回去重的按钮权限码。
    #[tokio::test]
    async fn access_codes_returns_super_flag_for_super_user() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;

        let result = get_access_codes(&db, actor.id).await;

        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;

        assert_eq!(
            result.unwrap(),
            vec![SUPER_ROLE_KEY.to_string()],
            "有效 super 角色应返回超管标识"
        );
    }

    #[tokio::test]
    async fn access_codes_returns_deduped_sorted_button_codes_for_normal_user() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let role_a = seed_role(&db, 1, None).await;
        let role_b = seed_role(&db, 1, None).await;
        let menu_create = seed_button(&db, "system:user:create").await;
        let menu_update = seed_button(&db, "system:user:update").await;

        bind_user_role(&db, actor.id, role_a.id).await;
        bind_user_role(&db, actor.id, role_b.id).await;
        bind_role_menu(&db, role_a.id, menu_create.id).await;
        bind_role_menu(&db, role_a.id, menu_update.id).await;
        // 两个角色都绑定同一按钮，验证去重。
        bind_role_menu(&db, role_b.id, menu_create.id).await;

        let result = get_access_codes(&db, actor.id).await;

        cleanup_actor(
            &db,
            &actor,
            &[role_a, role_b],
            &[menu_create, menu_update],
            None,
        )
        .await;

        assert_eq!(
            result.unwrap(),
            vec![
                "system:user:create".to_string(),
                "system:user:update".to_string()
            ],
            "应去重并按稳定顺序返回按钮权限码"
        );
    }

    #[tokio::test]
    async fn access_codes_returns_empty_for_user_without_permissions() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let role = seed_role(&db, 1, None).await;
        bind_user_role(&db, actor.id, role.id).await;

        let result = get_access_codes(&db, actor.id).await;

        cleanup_actor(&db, &actor, &[role], &[], None).await;

        assert!(result.unwrap().is_empty(), "无按钮权限的用户应返回空数组");
    }

    #[tokio::test]
    async fn access_codes_returns_empty_for_user_without_roles() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;

        let result = get_access_codes(&db, actor.id).await;

        cleanup_actor(&db, &actor, &[], &[], None).await;

        assert!(
            result.unwrap().is_empty(),
            "无角色用户应返回空数组，且不触发无效 SQL"
        );
    }

    // ---- update_user（admin 保护 + 权限校验）----

    /// 无权限操作者调用 update 被拒，且目标用户数据不变。
    #[tokio::test]
    async fn update_user_denies_actor_without_permission_and_saves_nothing() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let role = seed_role(&db, 1, None).await;
        bind_user_role(&db, actor.id, role.id).await;

        let target = seed_target_user(&db, unique_name("no_perm_update")).await;

        // 无权限操作者：普通角色，无 system:user:update 按钮。
        let no_perm_actor = seed_actor(&db, None).await;
        let no_perm_role = seed_role(&db, 1, None).await;
        bind_user_role(&db, no_perm_actor.id, no_perm_role.id).await;

        let nickname_before = target.nickname.clone();
        let result = update_user_with_links(
            &db,
            no_perm_actor.id,
            update_request(target.id, target.username.clone(), vec![]),
        )
        .await;

        let untouched = user_repo::find_by_id(&db, target.id)
            .await
            .unwrap()
            .expect("目标用户仍应存在");

        cleanup_actor(&db, &no_perm_actor, &[no_perm_role], &[], None).await;
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(target.id))
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(target.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(
            result.is_err(),
            "无 system:user:update 权限应被拒绝: {result:?}"
        );
        assert_eq!(
            untouched.nickname, nickname_before,
            "无权限更新时目标用户不应被修改"
        );
    }

    /// 拥有 system:user:update 按钮权限的操作者可更新普通用户（含角色重绑与密码更新）。
    #[tokio::test]
    async fn update_user_allows_actor_with_button_permission() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let role = seed_role(&db, 1, None).await;
        let menu = seed_button(&db, SYSTEM_USER_UPDATE).await;
        bind_user_role(&db, actor.id, role.id).await;
        bind_role_menu(&db, role.id, menu.id).await;

        let assign_role = sys_role::ActiveModel {
            role_name: Set("更新后分配角色".to_string()),
            role_key: Set(unique_name("update_assign_role")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let target = seed_target_user(&db, unique_name("perm_update")).await;

        let mut req = update_request(target.id, target.username.clone(), vec![assign_role.id]);
        req.password = "newpass".to_string();
        req.nickname = "更新后的昵称".to_string();

        let result = update_user_with_links(&db, actor.id, req).await;
        let updated = result.expect("拥有 system:user:update 的普通用户应更新成功");

        let saved = user_repo::find_by_id(&db, updated.id)
            .await
            .unwrap()
            .expect("更新后的用户应仍存在");
        let links = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(updated.id))
            .all(&db)
            .await
            .unwrap();

        // 清理：目标用户、分配角色、actor 及绑定。
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(updated.id))
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(updated.id)
            .exec(&db)
            .await
            .unwrap();
        sys_role::Entity::delete_by_id(assign_role.id)
            .exec(&db)
            .await
            .unwrap();
        cleanup_actor(&db, &actor, &[role], &[menu], None).await;

        assert_eq!(saved.nickname, "更新后的昵称");
        assert_eq!(saved.emp_no, "T2001");
        assert!(
            saved.password.starts_with("$argon2id$"),
            "非空密码应重新哈希"
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].role_id, assign_role.id, "角色关联应重绑为新角色");
    }

    /// 更新内置超管 admin 被拒，且 admin 数据不变。
    #[tokio::test]
    async fn update_user_rejects_builtin_admin() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;

        let (admin, owned_by_test) = load_or_create_admin(&db).await;
        let admin_before = user_repo::find_by_id(&db, admin.id)
            .await
            .unwrap()
            .expect("admin 应已保存");

        let result = update_user_with_links(
            &db,
            actor.id,
            update_request(admin.id, admin.username.clone(), vec![]),
        )
        .await;

        let admin_after = user_repo::find_by_id(&db, admin.id)
            .await
            .unwrap()
            .expect("admin 应仍存在");

        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;
        if owned_by_test {
            sys_user::Entity::delete_by_id(admin.id)
                .exec(&db)
                .await
                .unwrap();
        }

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("管理员不允许编辑")),
            "内置超管应拒绝编辑: {result:?}"
        );
        assert_eq!(
            admin_after.password, admin_before.password,
            "admin 密码不应被修改"
        );
        assert_eq!(
            admin_after.status, admin_before.status,
            "admin 状态不应被修改"
        );
    }

    /// 更新普通用户时，把用户名改成 admin 被拒。
    #[tokio::test]
    async fn update_user_rejects_renaming_to_admin() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;

        let target = {
            let created = create_user(&db, actor.id, request(unique_name("to_admin"), vec![]))
                .await
                .unwrap();
            let saved = user_repo::find_by_id(&db, created.id)
                .await
                .unwrap()
                .expect("目标用户应已保存");
            saved
        };

        let result = update_user_with_links(
            &db,
            actor.id,
            update_request(target.id, ADMIN_USERNAME.to_string(), vec![]),
        )
        .await;

        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(target.id))
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(target.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("不能与系统内置管理员相同")),
            "普通用户不应允许顶替 admin: {result:?}"
        );
    }

    /// 更新普通用户时，用户名与他人重复（排除自身）被拒。
    #[tokio::test]
    async fn update_user_rejects_duplicate_username_excluding_self() {
        let db = test_db().await;
        let actor = seed_actor(&db, None).await;
        let super_fixture = load_super_role(&db).await;
        bind_user_role(&db, actor.id, super_fixture.role.id).await;

        let other = {
            let created = create_user(&db, actor.id, request(unique_name("dup_owner"), vec![]))
                .await
                .unwrap();
            let saved = user_repo::find_by_id(&db, created.id)
                .await
                .unwrap()
                .expect("占用用户名用户应已保存");
            saved
        };
        let target = {
            let created = create_user(&db, actor.id, request(unique_name("dup_self"), vec![]))
                .await
                .unwrap();
            let saved = user_repo::find_by_id(&db, created.id)
                .await
                .unwrap()
                .expect("目标用户应已保存");
            saved
        };

        let result = update_user_with_links(
            &db,
            actor.id,
            update_request(target.id, other.username.clone(), vec![]),
        )
        .await;

        let other_after = user_repo::find_by_id(&db, other.id)
            .await
            .unwrap()
            .expect("占用用户名用户应仍存在");
        let target_after = user_repo::find_by_id(&db, target.id)
            .await
            .unwrap()
            .expect("目标用户应仍存在");

        cleanup_actor(&db, &actor, &[], &[], Some(super_fixture)).await;
        for user_id in [target.id, other.id] {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(user_id))
                .exec(&db)
                .await
                .unwrap();
            sys_user::Entity::delete_by_id(user_id)
                .exec(&db)
                .await
                .unwrap();
        }

        assert!(
            matches!(result, Err(AppError::Biz(ref message)) if message.contains("用户名已存在")),
            "与他人重名应被拒绝: {result:?}"
        );
        assert_eq!(
            target_after.username, target.username,
            "目标用户名不应被修改"
        );
        assert_eq!(
            other_after.username, other.username,
            "占用用户名用户不应受影响"
        );
    }
}
