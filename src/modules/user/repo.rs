use crate::entity::sys_user::Model;
use crate::entity::{sys_role, sys_user, sys_user_role};
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, PaginatorTrait, TransactionTrait};

pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    Ok(sys_user::Entity::find_by_id(id)
        .filter(sys_user::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

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
/// 返回 `(总条数, 总页数, 当前页数据)`。
pub async fn find_page(
    db: &DatabaseConnection,
    keyword: Option<String>,
    status: Option<i8>,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<(u64, u64, Vec<Model>)> {
    let mut cond = Condition::all();
    if let Some(kw) = keyword {
        cond = cond.add(sys_user::Column::Username.like(format!("%{kw}%")));
    }
    if let Some(s) = status {
        cond = cond.add(sys_user::Column::Status.eq(s));
    }

    let paginator = sys_user::Entity::find()
        .filter(cond)
        .filter(sys_user::Column::DeletedAt.is_null())
        .paginate(db, page_size);
    let items_and_pages = paginator.num_items_and_pages().await?;
    let total = items_and_pages.number_of_items;
    let total_pages = items_and_pages.number_of_pages;
    let items = paginator.fetch_page(page_index).await?;
    Ok((total, total_pages, items))
}

pub async fn create_user_with_roles(
    db: &DatabaseConnection,
    user: sys_user::ActiveModel,
    role_ids: Vec<u64>,
) -> anyhow::Result<sys_user::Model> {
    let txn = db.begin().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, Database, Set};

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> sea_orm::DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    #[tokio::test]
    async fn test_find_by_username_found() {
        let db = test_db().await;
        let username = format!("test_user_{}", std::process::id()); // 唯一名，避免并行冲突

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
        let username = format!("page_user_{}", std::process::id()); // 唯一名

        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("分页测试".to_string()),
            ..Default::default()
        };
        let inserted = model.insert(&db).await.unwrap();

        // 关键词命中 + 0-based 第 0 页
        let (total, total_page, items) = find_page(&db, Some(username.clone()), None, 0, 10)
            .await
            .unwrap();
        assert!(total >= 1);
        assert!(items.iter().any(|u| u.username == username));

        // 关键词不命中
        let (total, total_page, _) =
            find_page(&db, Some("no_such_keyword_xyz".to_string()), None, 0, 10)
                .await
                .unwrap();
        assert_eq!(total, 0);

        sys_user::Entity::delete_by_id(inserted.id)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_user_with_roles_supports_empty_role_ids() {
        let db = test_db().await;
        let username = format!("create_user_empty_roles_{}", std::process::id());

        let created = create_user_with_roles(
            &db,
            sys_user::ActiveModel {
                username: Set(username.clone()),
                password: Set("hashed-password".to_string()),
                nickname: Set("空角色创建".to_string()),
                ..Default::default()
            },
            vec![],
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
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
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
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
        mark_deleted.update(&db).await.unwrap();

        let result = find_page(&db, Some(keyword), None, 0, 10).await;

        sys_user::Entity::delete_by_id(live.id)
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(deleted.id)
            .exec(&db)
            .await
            .unwrap();

        let (total, total_page, items) = result.unwrap();
        assert_eq!(total, 1);
        assert!(items.iter().any(|user| user.username == live_username));
        assert!(
            !items.iter().any(|user| user.username == deleted_username),
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
        mark_deleted.deleted_at = Set(Some(chrono::Utc::now().naive_utc()));
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
}
