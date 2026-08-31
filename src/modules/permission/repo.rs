//! 权限数据访问层。

use sea_orm::entity::prelude::*;
use sea_orm::{DatabaseConnection, QueryOrder, QuerySelect};

use crate::entity::{sys_menu, sys_role_menu};
use crate::modules::user::repo as user_repo;

/// 查询用户当前真实有效的按钮权限码。
///
/// 返回结果应去重；调用方如需稳定展示，可再做排序。
pub async fn find_permission_codes_by_user_id(
    db: &DatabaseConnection,
    user_id: u64,
) -> anyhow::Result<Vec<String>> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;

    let role_ids = roles.into_iter().map(|role| role.id).collect::<Vec<_>>();
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    let role_menu_ids = sys_role_menu::Entity::find()
        .filter(sys_role_menu::Column::RoleId.is_in(role_ids))
        .column(sys_role_menu::Column::MenuId)
        .all(db)
        .await?;

    let menu_ids = role_menu_ids
        .into_iter()
        .map(|role_menu| role_menu.menu_id)
        .collect::<Vec<_>>();
    if menu_ids.is_empty() {
        return Ok(Vec::new());
    }

    let menus = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.is_in(menu_ids))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .filter(sys_menu::Column::Status.eq(1))
        .filter(sys_menu::Column::Permission.ne(""))
        // 仅查询menu类型 = 3 的为按钮
        .filter(sys_menu::Column::MenuType.eq(3))
        .order_by_asc(sys_menu::Column::Id)
        .all(db)
        .await?;

    let mut permission_codes = menus
        .into_iter()
        .map(|menu| menu.permission)
        .collect::<Vec<_>>();

    permission_codes.sort();
    permission_codes.dedup();

    Ok(permission_codes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_menu, sys_role, sys_role_menu, sys_user, sys_user_role};
    use crate::modules::permission::SUPER_ROLE_KEY;
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 已存在的 `super` 是全局唯一角色，测试只能复用；只有新创建时才物理清理。
    struct SuperRoleFixture {
        role: sys_role::Model,
        owned_by_test: bool,
    }

    #[derive(Default)]
    struct Fixture {
        user: Option<sys_user::Model>,
        roles: Vec<sys_role::Model>,
        menus: Vec<sys_menu::Model>,
        super_role: Option<SuperRoleFixture>,
    }

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
            remark: Set("权限测试创建".to_string()),
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

    async fn seed_user(
        db: &DatabaseConnection,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique("perm_user")),
            password: Set("x".to_string()),
            nickname: Set("权限测试用户".to_string()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(
        db: &DatabaseConnection,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("perm_role")),
            role_key: Set(unique("perm_role_key")),
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

    async fn seed_button(
        db: &DatabaseConnection,
        permission: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_menu::Model {
        seed_menu(db, permission, status, deleted_at, 3).await
    }

    async fn seed_menu(
        db: &DatabaseConnection,
        permission: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
        menu_type: i8,
    ) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(0),
            title: Set(unique("perm_menu")),
            name: Set(unique("PermMenu")),
            menu_type: Set(menu_type),
            permission: Set(permission.to_string()),
            status: Set(status),
            deleted_at: Set(deleted_at),
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

    async fn cleanup(db: &DatabaseConnection, fixture: Fixture) {
        if let Some(user) = fixture.user.as_ref() {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(user.id))
                .exec(db)
                .await
                .unwrap();
            sys_user::Entity::delete_by_id(user.id)
                .exec(db)
                .await
                .unwrap();
        }

        for role in &fixture.roles {
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

        for menu in &fixture.menus {
            sys_menu::Entity::delete_by_id(menu.id)
                .exec(db)
                .await
                .unwrap();
        }

        if let Some(super_role) = fixture.super_role.filter(|item| item.owned_by_test) {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(super_role.role.id))
                .exec(db)
                .await
                .unwrap();
            sys_role::Entity::delete_by_id(super_role.role.id)
                .exec(db)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn find_permission_codes_returns_bound_active_button() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let role = seed_role(&db, 1, None).await;
        let menu = seed_button(&db, "system:user:create", 1, None).await;
        bind_user_role(&db, user.id, role.id).await;
        bind_role_menu(&db, role.id, menu.id).await;

        let result = find_permission_codes_by_user_id(&db, user.id).await;

        cleanup(
            &db,
            Fixture {
                user: Some(user),
                roles: vec![role],
                menus: vec![menu],
                ..Default::default()
            },
        )
        .await;

        let codes = result.unwrap();
        assert!(codes.contains(&"system:user:create".to_string()));
    }

    #[tokio::test]
    async fn find_permission_codes_excludes_disabled_and_deleted_roles() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let active_role = seed_role(&db, 1, None).await;
        let disabled_role = seed_role(&db, 0, None).await;
        let deleted_role = seed_role(&db, 1, Some(chrono::Utc::now().naive_utc())).await;
        let menu = seed_button(&db, "system:user:create", 1, None).await;

        for role in [&active_role, &disabled_role, &deleted_role] {
            bind_user_role(&db, user.id, role.id).await;
            bind_role_menu(&db, role.id, menu.id).await;
        }

        let result = find_permission_codes_by_user_id(&db, user.id).await;

        cleanup(
            &db,
            Fixture {
                user: Some(user),
                roles: vec![active_role, disabled_role, deleted_role],
                menus: vec![menu],
                ..Default::default()
            },
        )
        .await;

        let codes = result.unwrap();
        assert!(codes.contains(&"system:user:create".to_string()));
        // 只有一个有效来源；如果出现两个，说明禁用/删除角色未被过滤。
        assert_eq!(
            codes
                .iter()
                .filter(|code| **code == "system:user:create")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn find_permission_codes_excludes_invalid_buttons() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let role = seed_role(&db, 1, None).await;
        let active_menu = seed_button(&db, "system:user:update", 1, None).await;
        let disabled_menu = seed_button(&db, "system:user:create", 0, None).await;
        let deleted_menu = seed_button(
            &db,
            "system:user:create",
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;
        let empty_permission_menu = seed_button(&db, "", 1, None).await;

        bind_user_role(&db, user.id, role.id).await;
        for menu in [
            &active_menu,
            &disabled_menu,
            &deleted_menu,
            &empty_permission_menu,
        ] {
            bind_role_menu(&db, role.id, menu.id).await;
        }

        let result = find_permission_codes_by_user_id(&db, user.id).await;

        cleanup(
            &db,
            Fixture {
                user: Some(user),
                roles: vec![role],
                menus: vec![
                    active_menu,
                    disabled_menu,
                    deleted_menu,
                    empty_permission_menu,
                ],
                ..Default::default()
            },
        )
        .await;

        let codes = result.unwrap();
        assert_eq!(codes, vec!["system:user:update"]);
    }

    #[tokio::test]
    async fn find_permission_codes_only_accepts_button_type() {
        let db = test_db().await;
        let user = seed_user(&db, 1, None).await;
        let role = seed_role(&db, 1, None).await;
        let button = seed_menu(&db, "system:user:create", 1, None, 3).await;
        let directory = seed_menu(&db, "system:user:create", 1, None, 1).await;

        bind_user_role(&db, user.id, role.id).await;
        bind_role_menu(&db, role.id, button.id).await;
        bind_role_menu(&db, role.id, directory.id).await;

        let result = find_permission_codes_by_user_id(&db, user.id).await;

        cleanup(
            &db,
            Fixture {
                user: Some(user),
                roles: vec![role],
                menus: vec![button, directory],
                ..Default::default()
            },
        )
        .await;

        let codes = result.unwrap();
        assert_eq!(
            codes
                .iter()
                .filter(|code| **code == "system:user:create")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn super_owner_has_permission_without_binding_a_button() {
        let db = test_db().await;
        let super_role_fixture = load_super_role(&db).await;
        let user = seed_user(&db, 1, None).await;
        bind_user_role(&db, user.id, super_role_fixture.role.id).await;

        let result =
            super::super::service::has_permission(&db, user.id, "anything:not:listed").await;

        cleanup(
            &db,
            Fixture {
                user: Some(user),
                super_role: Some(SuperRoleFixture {
                    role: super_role_fixture.role,
                    owned_by_test: super_role_fixture.owned_by_test,
                }),
                ..Default::default()
            },
        )
        .await;

        assert!(result.unwrap());
    }
}
