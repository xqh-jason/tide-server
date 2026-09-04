use crate::entity::sys_user::Model;
use crate::entity::{sys_role, sys_user, sys_user_role};
use crate::modules::user::dto::UserFilter;
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, TransactionTrait};

/// 查询单个有效用户（排除软删除）。
pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    Ok(sys_user::Entity::find_by_id(id)
        .filter(sys_user::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按用户名查询有效用户（登录用，排除软删除）。
pub async fn find_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<Model>> {
    let user = sys_user::Entity::find()
        .filter(sys_user::Column::Username.eq(username))
        .filter(sys_user::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    Ok(user)
}

/// 按用户名查询（含软删占位，创建用户查重用）。
pub async fn find_by_username_include_deleted(
    db: &DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<Model>> {
    let user = sys_user::Entity::find()
        .filter(sys_user::Column::Username.eq(username))
        .one(db)
        .await?;

    Ok(user)
}

/// 查询用户关联的所有启用角色（W2 登录取角色；先不做 join，W3 学 many-to-many 时再换 Linked）。
pub async fn find_roles_by_user_id(
    db: &DatabaseConnection,
    user_id: u64,
) -> anyhow::Result<Vec<sys_role::Model>> {
    let links = sys_user_role::Entity::find()
        .filter(sys_user_role::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    if links.is_empty() {
        return Ok(vec![]);
    }
    let role_ids: Vec<u64> = links.into_iter().map(|l| l.role_id).collect();
    Ok(sys_role::Entity::find()
        .filter(sys_role::Column::Id.is_in(role_ids))
        .filter(sys_role::Column::Status.eq(1))
        .filter(sys_role::Column::DeletedAt.is_null())
        .all(db)
        .await?)
}

/// 分页 + 动态过滤查询（列表接口核心）：
/// - 过滤条件用 `Condition` 动态拼接（有值才 add，无值跳过）
/// - 分页用 `PaginatorTrait::paginate`，`page_index` 为 0-based
/// 返回 `PageData<Model>`（总条数 / 总页数 / 当前页数据）。
pub async fn find_page(
    db: &DatabaseConnection,
    filter: &UserFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(kw) = &filter.keyword {
        cond = cond.add(sys_user::Column::Username.like(format!("%{kw}%")));
    }
    if let Some(s) = filter.status {
        cond = cond.add(sys_user::Column::Status.eq(s));
    }

    let select = sys_user::Entity::find()
        .filter(cond)
        .filter(sys_user::Column::DeletedAt.is_null());
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 事务创建用户并批量绑定角色关联（`role_ids` 为空则不建关联）。
///
/// `actor_id` 为操作人，审计字段（`created_by` / `updated_by`）由 repo 统一盖章，
/// 调用方无需在 ActiveModel 中设置。
pub async fn create_user_with_links(
    db: &DatabaseConnection,
    user: sys_user::ActiveModel,
    role_ids: Vec<u64>,
    actor_id: u64,
) -> anyhow::Result<sys_user::Model> {
    let txn = db.begin().await?;
    // 创建场景：创建人与更新人同源
    let mut user = user;
    user.created_by = Set(actor_id);
    user.updated_by = Set(actor_id);
    let user = user.insert(&txn).await?;

    if !role_ids.is_empty() {
        let user_role_ids = role_ids
            .into_iter()
            .map(|role_id| sys_user_role::ActiveModel {
                user_id: Set(user.id),
                role_id: Set(role_id),
            });

        sys_user_role::Entity::insert_many(user_role_ids)
            .exec(&txn)
            .await?;
    }

    txn.commit().await?;

    Ok(user)
}

/// 事务更新用户并重建角色关联（先删旧关联，再插新关联）。
pub async fn update_user_with_links(
    db: &DatabaseConnection,
    user: sys_user::ActiveModel,
    role_ids: Vec<u64>,
    actor_id: u64,
) -> anyhow::Result<sys_user::Model> {
    let txn = db.begin().await?;
    // 更新场景：只刷新更新人；created_by 保持 NotSet，不会被覆盖
    let mut user = user;
    user.updated_by = Set(actor_id);
    let user = user.update(&txn).await?;

    if !role_ids.is_empty() {
        // 先删除旧角色关联
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(user.id))
            .exec(&txn)
            .await?;

        // 再插入新角色关联
        let user_role_ids = role_ids
            .into_iter()
            .map(|role_id| sys_user_role::ActiveModel {
                user_id: Set(user.id),
                role_id: Set(role_id),
            });

        sys_user_role::Entity::insert_many(user_role_ids)
            .exec(&txn)
            .await?;
    }

    txn.commit().await?;

    Ok(user)
}

/// 通用更新（ActiveModel 入参，状态更新等单字段场景用）。
pub async fn update_user(
    db: &DatabaseConnection,
    model: sys_user::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<bool> {
    // 通用更新同样盖章：只刷新更新人
    let mut model = model;
    model.updated_by = Set(actor_id);
    model.update(db).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, Database, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    /// 唯一命名：`<prefix>_<pid>_<seq>`，避免并行测试冲突。
    fn unique_name(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> sea_orm::DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    #[tokio::test]
    async fn test_find_by_username_found() {
        let db = test_db().await;
        let username = unique_name("test_user");

        // 1. 插入测试数据（ActiveModel 写入，NotSet 字段保持默认）
        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("测试用户".to_string()),
            ..Default::default()
        };
        let inserted = model.insert(&db).await.unwrap();

        // 2. 查询并断言
        let found = find_by_username(&db, &username).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, username);

        // 3. 清理（delete_by_id 是 Entity 的方法）
        sys_user::Entity::delete_by_id(inserted.id)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_find_by_username_not_found() {
        let db = test_db().await;
        let found = find_by_username(&db, "definitely_not_exists_xyz")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_page() {
        let db = test_db().await;
        let username = unique_name("page_user");

        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("分页测试".to_string()),
            ..Default::default()
        };
        let inserted = model.insert(&db).await.unwrap();

        // 关键词命中 + 0-based 第 0 页
        let data = find_page(
            &db,
            &UserFilter {
                keyword: Some(username.clone()),
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert!(data.total >= 1);
        assert!(data.items.iter().any(|u| u.username == username));

        // 关键词不命中
        let data = find_page(
            &db,
            &UserFilter {
                keyword: Some("no_such_keyword_xyz".to_string()),
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(data.total, 0);

        sys_user::Entity::delete_by_id(inserted.id)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_user_with_links_supports_empty_role_ids() {
        let db = test_db().await;
        let username = unique_name("create_user_empty_roles");

        let created = create_user_with_links(
            &db,
            sys_user::ActiveModel {
                username: Set(username.clone()),
                password: Set("hashed-password".to_string()),
                nickname: Set("空角色创建".to_string()),
                ..Default::default()
            },
            vec![],
            ACTOR_ID,
        )
        .await
        .unwrap();

        let found = find_by_username(&db, &username)
            .await
            .unwrap()
            .expect("用户应已创建");
        assert_eq!(found.id, created.id);

        let links = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&db)
            .await
            .unwrap();
        assert!(links.is_empty());

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

    #[tokio::test]
    async fn find_by_username_ignores_deleted_user() {
        let db = test_db().await;
        let username = format!("deleted_find_user_{}", std::process::id());

        let inserted = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("hashed-password".to_string()),
            nickname: Set("软删除查询用户".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut mark_deleted: sys_user::ActiveModel = inserted.clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        mark_deleted.update(&db).await.unwrap();

        let found = find_by_username(&db, &username).await.unwrap();

        // 无论当前实现是否过滤软删除，都先物理清理测试数据，避免失败时残留。
        sys_user::Entity::delete_by_id(inserted.id)
            .exec(&db)
            .await
            .unwrap();

        assert!(found.is_none(), "已删除用户不应被普通业务查询找到");
    }

    #[tokio::test]
    async fn find_page_excludes_deleted_users() {
        let db = test_db().await;
        let keyword = format!("soft_delete_page_{}", std::process::id());
        let live_username = format!("{keyword}_live");
        let deleted_username = format!("{keyword}_deleted");

        let live = sys_user::ActiveModel {
            username: Set(live_username.clone()),
            password: Set("x".to_string()),
            nickname: Set("正常分页用户".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let deleted = sys_user::ActiveModel {
            username: Set(deleted_username.clone()),
            password: Set("x".to_string()),
            nickname: Set("已删除分页用户".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut mark_deleted: sys_user::ActiveModel = deleted.clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        mark_deleted.update(&db).await.unwrap();

        let result = find_page(
            &db,
            &UserFilter {
                keyword: Some(keyword),
                status: None,
            },
            0,
            10,
        )
        .await;

        sys_user::Entity::delete_by_id(live.id)
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(deleted.id)
            .exec(&db)
            .await
            .unwrap();

        let data = result.unwrap();
        assert_eq!(data.total, 1);
        assert!(data.items.iter().any(|user| user.username == live_username));
        assert!(
            !data
                .items
                .iter()
                .any(|user| user.username == deleted_username),
            "用户分页不应包含软删除记录"
        );
    }

    #[tokio::test]
    async fn find_roles_by_user_id_excludes_disabled_and_deleted_roles() {
        let db = test_db().await;
        let username = format!("role_filter_user_{}", std::process::id());

        let user = sys_user::ActiveModel {
            username: Set(username),
            password: Set("x".to_string()),
            nickname: Set("角色过滤测试用户".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let enabled_key = format!("enabled_{}", std::process::id());
        let disabled_key = format!("disabled_{}", std::process::id());
        let deleted_key = format!("deleted_{}", std::process::id());

        let mut roles = Vec::new();
        for (key, status) in [
            (enabled_key.clone(), 1),
            (disabled_key.clone(), 0),
            (deleted_key.clone(), 1),
        ] {
            roles.push(
                sys_role::ActiveModel {
                    role_name: Set(key.clone()),
                    role_key: Set(key),
                    sort: Set(0),
                    status: Set(status),
                    remark: Set(String::new()),
                    ..Default::default()
                }
                .insert(&db)
                .await
                .unwrap(),
            );
        }

        for role in &roles {
            sys_user_role::ActiveModel {
                user_id: Set(user.id),
                role_id: Set(role.id),
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let mut mark_deleted: sys_role::ActiveModel = roles[2].clone().into();
        mark_deleted.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        mark_deleted.update(&db).await.unwrap();

        let found_keys = find_roles_by_user_id(&db, user.id)
            .await
            .unwrap()
            .into_iter()
            .map(|role| role.role_key)
            .collect::<Vec<_>>();

        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(user.id))
            .exec(&db)
            .await
            .unwrap();
        for role in roles {
            sys_role::Entity::delete_by_id(role.id)
                .exec(&db)
                .await
                .unwrap();
        }
        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();

        assert_eq!(found_keys.len(), 1);
        assert!(found_keys.contains(&enabled_key));
        assert!(!found_keys.contains(&disabled_key));
        assert!(
            !found_keys.contains(&deleted_key),
            "登录角色不应包含已删除角色"
        );
    }

    /// 造一个操作人用户（直接 insert，不走 repo；其审计字段为 NULL 属预期）。
    async fn seed_actor(db: &DatabaseConnection) -> u64 {
        sys_user::ActiveModel {
            username: Set(unique_name("audit_actor")),
            password: Set("x".to_string()),
            nickname: Set("审计操作人".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    /// 清理：先删关联表，再删主表（含操作人）。
    async fn cleanup_users(db: &DatabaseConnection, ids: &[u64]) {
        for id in ids {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(*id))
                .exec(db)
                .await
                .unwrap();
        }
        for id in ids {
            sys_user::Entity::delete_by_id(*id).exec(db).await.unwrap();
        }
    }

    #[tokio::test]
    async fn create_user_with_links_stamps_actor_as_creator_and_updater() {
        let db = test_db().await;
        let actor_id = seed_actor(&db).await;

        let created = create_user_with_links(
            &db,
            sys_user::ActiveModel {
                username: Set(unique_name("audit_target")),
                password: Set("x".to_string()),
                nickname: Set("审计目标用户".to_string()),
                ..Default::default()
            },
            vec![],
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id);
        assert_eq!(created.updated_by, actor_id);

        cleanup_users(&db, &[created.id, actor_id]).await;
    }

    #[tokio::test]
    async fn update_user_with_links_refreshes_updated_by_and_keeps_created_by() {
        let db = test_db().await;
        let creator_id = seed_actor(&db).await;
        let updater_id = seed_actor(&db).await;

        let created = create_user_with_links(
            &db,
            sys_user::ActiveModel {
                username: Set(unique_name("audit_target")),
                password: Set("x".to_string()),
                nickname: Set("审计目标用户".to_string()),
                ..Default::default()
            },
            vec![],
            creator_id,
        )
        .await
        .unwrap();

        // 换另一个操作人做局部更新：updated_by 应刷新，created_by 保持原值
        let updated = update_user_with_links(
            &db,
            sys_user::ActiveModel {
                id: Set(created.id),
                nickname: Set("改名后".to_string()),
                ..Default::default()
            },
            vec![],
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);

        // 通用更新（状态更新场景）同样要盖章
        let mut model: sys_user::ActiveModel = updated.into();
        model.status = Set(0);
        let after_status = update_user(&db, model, updater_id).await.unwrap();
        assert!(after_status);
        let reloaded = find_by_id(&db, created.id).await.unwrap().unwrap();
        assert_eq!(reloaded.updated_by, updater_id);

        cleanup_users(&db, &[created.id, creator_id, updater_id]).await;
    }
}
