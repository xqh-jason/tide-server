//! 开发环境种子数据：启动时幂等执行（可重复运行，不重复插入）。
//!
//! 内容：admin 用户、super 角色、默认菜单树（目录/页面/按钮权限码）、
//! admin-super 绑定、super 全量菜单绑定。
//! ⚠️ 开发约定：admin 密码固定为 `admin123`（每次启动重置）；生产环境请勿挂载本初始化。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::entity::{sys_menu, sys_role, sys_role_menu, sys_user, sys_user_role};
use crate::utils::crypt;

/// admin 初始密码（开发环境约定）。
pub const SEED_ADMIN_PASSWORD: &str = "admin123";
/// admin 用户名。
pub const SEED_ADMIN_USERNAME: &str = "admin";
/// 超级管理员角色键（与 `permission::SUPER_ROLE_KEY` 同值，避免依赖方向循环）。
const SEED_SUPER_ROLE_KEY: &str = "super";

/// 播种互斥锁：ensure_seed 的"先查后插"在并发下不幂等（TOCTOU），
/// 启动初始化与测试并发调用时通过进程内锁串行化。
static SEED_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

/// 菜单种子定义：`parent` 引用父菜单的 `name`（顶层为 `None`）。
struct MenuSeed {
    name: &'static str,
    title: &'static str,
    path: &'static str,
    component: &'static str,
    icon: &'static str,
    menu_type: i8,
    permission: &'static str,
    parent: Option<&'static str>,
    sort: i32,
}

/// 默认菜单树（vben 契约：顶级 path 以 `/` 开头、component 可被 glob 命中、
/// name 全局唯一；menu_type：1 目录 / 2 页面 / 3 按钮）。
const MENU_SEEDS: &[MenuSeed] = &[
    // 系统管理目录
    MenuSeed {
        name: "System",
        title: "系统管理",
        path: "/system",
        component: "",
        icon: "lucide:settings",
        menu_type: 1,
        permission: "",
        parent: None,
        sort: 1,
    },
    // 用户管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemUser",
        title: "用户管理",
        path: "/system/user",
        component: "#/views/system/user/index.vue",
        icon: "lucide:users",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemUserCreate",
        title: "用户新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:user:create",
        parent: Some("SystemUser"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemUserUpdate",
        title: "用户修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:user:update",
        parent: Some("SystemUser"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemUserDelete",
        title: "用户删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:user:delete",
        parent: Some("SystemUser"),
        sort: 3,
    },
    // 角色管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemRole",
        title: "角色管理",
        path: "/system/role",
        component: "#/views/system/role/index.vue",
        icon: "lucide:shield",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemRoleCreate",
        title: "角色新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:role:create",
        parent: Some("SystemRole"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemRoleUpdate",
        title: "角色修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:role:update",
        parent: Some("SystemRole"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemRoleDelete",
        title: "角色删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:role:delete",
        parent: Some("SystemRole"),
        sort: 3,
    },
    // 菜单管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemMenu",
        title: "菜单管理",
        path: "/system/menu",
        component: "#/views/system/menu/index.vue",
        icon: "lucide:menu",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 3,
    },
    MenuSeed {
        name: "SystemMenuCreate",
        title: "菜单新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:menu:create",
        parent: Some("SystemMenu"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemMenuUpdate",
        title: "菜单修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:menu:update",
        parent: Some("SystemMenu"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemMenuDelete",
        title: "菜单删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:menu:delete",
        parent: Some("SystemMenu"),
        sort: 3,
    },
    // API 管理页面 + 按钮权限码
    MenuSeed {
        name: "SystemApi",
        title: "API 管理",
        path: "/system/api",
        component: "#/views/system/api/index.vue",
        icon: "lucide:webhook",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 4,
    },
    MenuSeed {
        name: "SystemApiCreate",
        title: "API 新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:api:create",
        parent: Some("SystemApi"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemApiUpdate",
        title: "API 修改",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:api:update",
        parent: Some("SystemApi"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemApiDelete",
        title: "API 删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:api:delete",
        parent: Some("SystemApi"),
        sort: 3,
    },
    // 操作日志页面 + 删除权限码（W5-1）
    MenuSeed {
        name: "SystemOperationLog",
        title: "操作日志",
        path: "/system/operation-log",
        component: "#/views/system/operation-log/index.vue",
        icon: "lucide:scroll-text",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 5,
    },
    MenuSeed {
        name: "SystemOperationLogDelete",
        title: "操作日志删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:operation-log:delete",
        parent: Some("SystemOperationLog"),
        sort: 1,
    },
];

/// 启动初始化：确保开发种子数据存在（幂等，可重复调用）。
pub async fn ensure_seed(db: &DatabaseConnection) -> anyhow::Result<()> {
    let lock = SEED_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;

    // 1. admin 用户：存在则重置密码（开发约定），不存在则创建
    let admin = sys_user::Entity::find()
        .filter(sys_user::Column::Username.eq(SEED_ADMIN_USERNAME))
        .one(db)
        .await?;
    let password_hash = crypt::hash_password(SEED_ADMIN_PASSWORD)?;
    let admin_id = if let Some(admin) = admin {
        let mut admin: sys_user::ActiveModel = admin.into();
        admin.password = Set(password_hash);
        admin.update(db).await?.id
    } else {
        sys_user::ActiveModel {
            username: Set(SEED_ADMIN_USERNAME.to_string()),
            password: Set(password_hash),
            emp_no: Set("admin".to_string()),
            nickname: Set("超级管理员".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await?
        .id
    };

    // 2. super 角色：不存在则创建
    let super_role = sys_role::Entity::find()
        .filter(sys_role::Column::RoleKey.eq(SEED_SUPER_ROLE_KEY))
        .one(db)
        .await?;
    let super_role_id = if let Some(role) = super_role {
        role.id
    } else {
        sys_role::ActiveModel {
            role_name: Set("超级管理员".to_string()),
            role_key: Set(SEED_SUPER_ROLE_KEY.to_string()),
            sort: Set(0),
            status: Set(1),
            remark: Set("系统内置超管角色".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await?
        .id
    };

    // 3. admin-super 绑定：不存在则创建
    let bound = sys_user_role::Entity::find()
        .filter(sys_user_role::Column::UserId.eq(admin_id))
        .filter(sys_user_role::Column::RoleId.eq(super_role_id))
        .one(db)
        .await?;
    if bound.is_none() {
        sys_user_role::ActiveModel {
            user_id: Set(admin_id),
            role_id: Set(super_role_id),
        }
        .insert(db)
        .await?;
    }

    // 4. 默认菜单：按 name 逐个补种，并记录 name → id 映射（按钮的 parent 引用）
    let mut menu_ids_by_name: std::collections::HashMap<&str, u64> = Default::default();
    for seed in MENU_SEEDS {
        let existing = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq(seed.name))
            .one(db)
            .await?;
        let menu_id = if let Some(menu) = existing {
            menu.id
        } else {
            let parent_id = seed.parent.map(|p| menu_ids_by_name[p]).unwrap_or(0);
            sys_menu::ActiveModel {
                parent_id: Set(parent_id),
                path: Set(seed.path.to_string()),
                name: Set(seed.name.to_string()),
                component: Set(seed.component.to_string()),
                title: Set(seed.title.to_string()),
                icon: Set(seed.icon.to_string()),
                sort: Set(seed.sort),
                keep_alive: Set(0),
                hidden: Set(0),
                menu_type: Set(seed.menu_type),
                permission: Set(seed.permission.to_string()),
                status: Set(1),
                ..Default::default()
            }
            .insert(db)
            .await?
            .id
        };
        menu_ids_by_name.insert(seed.name, menu_id);
    }

    // 5. super 角色绑定全部菜单：缺失的关联补上
    for menu_id in menu_ids_by_name.values() {
        let bound = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(super_role_id))
            .filter(sys_role_menu::Column::MenuId.eq(*menu_id))
            .one(db)
            .await?;
        if bound.is_none() {
            sys_role_menu::ActiveModel {
                role_id: Set(super_role_id),
                menu_id: Set(*menu_id),
            }
            .insert(db)
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DatabaseConnection};

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 种子完整性：admin 可登录、super 角色存在、默认菜单与绑定齐全。
    #[tokio::test]
    async fn ensure_seed_creates_admin_super_and_default_menus() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let admin = sys_user::Entity::find()
            .filter(sys_user::Column::Username.eq(SEED_ADMIN_USERNAME))
            .one(&db)
            .await
            .unwrap()
            .expect("admin 应存在");
        assert!(
            crypt::verify_password(SEED_ADMIN_PASSWORD, &admin.password),
            "admin 密码应可登录（admin123）"
        );

        let super_role = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq(SEED_SUPER_ROLE_KEY))
            .one(&db)
            .await
            .unwrap()
            .expect("super 角色应存在");
        assert!(
            sys_user_role::Entity::find()
                .filter(sys_user_role::Column::UserId.eq(admin.id))
                .filter(sys_user_role::Column::RoleId.eq(super_role.id))
                .one(&db)
                .await
                .unwrap()
                .is_some(),
            "admin-super 绑定应存在"
        );

        let menu_count = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.is_in(MENU_SEEDS.iter().map(|s| s.name.to_string())))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(menu_count as usize, MENU_SEEDS.len(), "全部默认菜单应存在");

        let role_menu_count = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(super_role.id))
            .count(&db)
            .await
            .unwrap();
        assert!(
            role_menu_count >= MENU_SEEDS.len() as u64,
            "super 应绑定全部默认菜单"
        );
    }

    /// 幂等：连续执行两次，默认菜单数量不变（不重复插入）。
    #[tokio::test]
    async fn ensure_seed_is_idempotent() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let count_before = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.is_in(MENU_SEEDS.iter().map(|s| s.name.to_string())))
            .count(&db)
            .await
            .unwrap();

        ensure_seed(&db).await.unwrap();

        let count_after = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.is_in(MENU_SEEDS.iter().map(|s| s.name.to_string())))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(count_before, count_after, "重复播种不应产生重复菜单");
    }

    /// W5-1：操作日志菜单页面与删除按钮权限码应随种子就绪。
    #[tokio::test]
    async fn ensure_seed_creates_operation_log_menu_and_button() {
        let db = test_db().await;
        ensure_seed(&db).await.unwrap();

        let menu = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("SystemOperationLog"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("操作日志菜单应存在");
        assert_eq!(menu.menu_type, 2, "操作日志应为页面菜单");
        assert_eq!(menu.title, "操作日志");
        assert_eq!(menu.path, "/system/operation-log");
        let system = sys_menu::Entity::find()
            .filter(sys_menu::Column::Name.eq("System"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("System 目录应存在");
        assert_eq!(menu.parent_id, system.id, "操作日志应挂在 System 目录下");

        let button = sys_menu::Entity::find()
            .filter(sys_menu::Column::Permission.eq("system:operation-log:delete"))
            .filter(sys_menu::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("操作日志删除按钮权限码应存在");
        assert_eq!(button.menu_type, 3, "按钮应为菜单类型 3");
        assert_eq!(button.parent_id, menu.id, "按钮应挂在操作日志菜单下");
    }
}
