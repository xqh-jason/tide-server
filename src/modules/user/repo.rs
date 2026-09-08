use crate::entity::sys_user::Model;
use crate::entity::{sys_role, sys_user, sys_user_role};
use crate::modules::user::dto::UserFilter;
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, DatabaseTransaction, QueryOrder};

/// 查询单个有效用户（排除软删除）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    Ok(sys_user::Entity::find_by_id(id)
        .filter(sys_user::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按用户名查询有效用户（登录用，排除软删除）。
pub async fn find_by_username(
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
///
/// 返回 `PageData<Model>`（总条数 / 总页数 / 当前页数据）。
pub async fn find_page(
    db: &impl ConnectionTrait,
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

    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_user::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_user::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_user::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_user::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_user::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_user::Column::UpdatedAt.lte(v));
    }
    let select = sys_user::Entity::find()
        .filter(cond)
        .filter(sys_user::Column::DeletedAt.is_null());
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 事务内实现：插入用户 + 角色关联（不 begin/commit，边界由调用方负责）。
pub(crate) async fn create_user_in_tx(
    txn: &DatabaseTransaction,
    user: sys_user::ActiveModel,
    role_ids: Vec<u64>,
    actor_id: u64,
) -> anyhow::Result<sys_user::Model> {
    // 创建场景：创建人与更新人同源
    let mut user = user;
    user.created_by = Set(actor_id);
    user.updated_by = Set(actor_id);
    let user = user.insert(txn).await?;

    if !role_ids.is_empty() {
        let user_role_ids = role_ids
            .into_iter()
            .map(|role_id| sys_user_role::ActiveModel {
                user_id: Set(user.id),
                role_id: Set(role_id),
            });

        sys_user_role::Entity::insert_many(user_role_ids)
            .exec(txn)
            .await?;
    }

    Ok(user)
}

/// 事务内实现：更新用户 + 重建角色关联（不 begin/commit，边界由调用方负责）。
pub(crate) async fn update_user_in_tx(
    txn: &DatabaseTransaction,
    user: sys_user::ActiveModel,
    role_ids: Vec<u64>,
    actor_id: u64,
) -> anyhow::Result<sys_user::Model> {
    // 更新场景：只刷新更新人；created_by 保持 NotSet，不会被覆盖
    let mut user = user;
    user.updated_by = Set(actor_id);
    let user = user.update(txn).await?;

    if !role_ids.is_empty() {
        // 先删除旧角色关联
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(user.id))
            .exec(txn)
            .await?;

        // 再插入新角色关联
        let user_role_ids = role_ids
            .into_iter()
            .map(|role_id| sys_user_role::ActiveModel {
                user_id: Set(user.id),
                role_id: Set(role_id),
            });

        sys_user_role::Entity::insert_many(user_role_ids)
            .exec(txn)
            .await?;
    }

    Ok(user)
}

/// 通用更新（ActiveModel 入参，状态更新等单字段场景用）。
pub async fn update_user(
    db: &impl ConnectionTrait,
    model: sys_user::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<bool> {
    // 通用更新同样盖章：只刷新更新人
    let mut model = model;
    model.updated_by = Set(actor_id);
    model.update(db).await?;
    Ok(true)
}

/// 事务内实现：清空角色关联 + 软删主表（不 begin/commit，边界由调用方负责）。
pub(crate) async fn soft_delete_user_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<()> {
    // 物理清空角色关联（关系表硬删除约定）
    sys_user_role::Entity::delete_many()
        .filter(sys_user_role::Column::UserId.eq(id))
        .exec(txn)
        .await?;

    // 软删主表（不存在或已软删时静默跳过，不报错）
    if let Some(user) = sys_user::Entity::find_by_id(id).one(txn).await? {
        let mut user: sys_user::ActiveModel = user.into();
        user.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        user.update(txn).await?;
    }

    Ok(())
}

/// 全量用户（**含软删**）：审计过滤的用户选择器数据源——历史记录的
/// 操作人即便已软删，仍需回显名字，故不过滤 `deleted_at`。
pub async fn find_all_include_deleted(db: &impl ConnectionTrait) -> anyhow::Result<Vec<Model>> {
    let models = sys_user::Entity::find()
        .order_by_asc(sys_user::Column::Id)
        .all(db)
        .await?;
    Ok(models)
}

/// 全量用户（仅排除软删），按 id 升序。
pub async fn find_all_users(db: &impl ConnectionTrait) -> anyhow::Result<Vec<Model>> {
    let models = sys_user::Entity::find()
        .filter(sys_user::Column::DeletedAt.is_null())
        .order_by_asc(sys_user::Column::Id)
        .all(db)
        .await?;
    Ok(models)
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

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }
    #[tokio::test]
    async fn test_find_by_username_found() {
        let db = test_txn().await;
        let username = unique_name("test_user");

        // 1. 插入测试数据（ActiveModel 写入，NotSet 字段保持默认）
        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("测试用户".to_string()),
            ..Default::default()
        };
        let _inserted = model.insert(&db).await.unwrap();

        // 2. 查询并断言
        let found = find_by_username(&db, &username).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, username);
    }

    #[tokio::test]
    async fn test_find_by_username_not_found() {
        let db = test_txn().await;
        let found = find_by_username(&db, "definitely_not_exists_xyz")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_page() {
        let db = test_txn().await;
        let username = unique_name("page_user");

        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("分页测试".to_string()),
            ..Default::default()
        };
        let _inserted = model.insert(&db).await.unwrap();

        // 关键词命中 + 0-based 第 0 页
        let data = find_page(
            &db,
            &UserFilter {
                keyword: Some(username.clone()),
                status: None,
                ..Default::default()
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
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(data.total, 0);
    }

    /// find_roles_by_user_id 只返回启用且未删的角色：停用/软删角色不贡献
    /// （此语义被 permission/menu 域的角色解析依赖，过滤必须发生在 user repo）。
    #[tokio::test]
    async fn find_roles_by_user_id_filters_disabled_and_deleted_roles() {
        let db = test_txn().await;
        let user = sys_user::ActiveModel {
            username: Set(unique_name("roles_user")),
            password: Set("x".to_string()),
            nickname: Set("角色过滤测试".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let active = sys_role::ActiveModel {
            role_name: Set(unique_name("roles_active")),
            role_key: Set(unique_name("roles_active_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let disabled = sys_role::ActiveModel {
            role_name: Set(unique_name("roles_disabled")),
            role_key: Set(unique_name("roles_disabled_key")),
            sort: Set(0),
            status: Set(0),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let deleted = sys_role::ActiveModel {
            role_name: Set(unique_name("roles_deleted")),
            role_key: Set(unique_name("roles_deleted_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            deleted_at: Set(Some(chrono::Local::now().naive_local())),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        for role_id in [active.id, disabled.id, deleted.id] {
            sys_user_role::ActiveModel {
                user_id: Set(user.id),
                role_id: Set(role_id),
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let roles = find_roles_by_user_id(&db, user.id).await.unwrap();

        let ids: Vec<u64> = roles.into_iter().map(|r| r.id).collect();
        assert!(ids.contains(&active.id), "启用未删角色应返回");
        assert!(!ids.contains(&disabled.id), "停用角色应被过滤");
        assert!(!ids.contains(&deleted.id), "软删角色应被过滤");
    }

    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn create_user_with_links_supports_empty_role_ids() {
        let txn = test_txn().await;
        let username = unique_name("create_user_empty_roles");

        let created = create_user_in_tx(
            &txn,
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

        let found = find_by_username(&txn, &username)
            .await
            .unwrap()
            .expect("用户应已创建");
        assert_eq!(found.id, created.id);

        let links = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        assert!(links.is_empty());
    }

    #[tokio::test]
    async fn find_by_username_ignores_deleted_user() {
        let db = test_txn().await;
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

        assert!(found.is_none(), "已删除用户不应被普通业务查询找到");
    }

    #[tokio::test]
    async fn find_page_excludes_deleted_users() {
        let db = test_txn().await;
        let keyword = format!("soft_delete_page_{}", std::process::id());
        let live_username = format!("{keyword}_live");
        let deleted_username = format!("{keyword}_deleted");

        let _live = sys_user::ActiveModel {
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
                ..Default::default()
            },
            0,
            10,
        )
        .await;

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
        let db = test_txn().await;
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

        assert_eq!(found_keys.len(), 1);
        assert!(found_keys.contains(&enabled_key));
        assert!(!found_keys.contains(&disabled_key));
        assert!(
            !found_keys.contains(&deleted_key),
            "登录角色不应包含已删除角色"
        );
    }

    /// 造一个操作人用户（直接 insert，不走 repo；其审计字段为 NULL 属预期）。
    async fn seed_actor(db: &impl ConnectionTrait) -> u64 {
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

    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn create_user_with_links_stamps_actor_as_creator_and_updater() {
        let txn = test_txn().await;
        let actor_id = seed_actor(&txn).await;

        let created = create_user_in_tx(
            &txn,
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
    }

    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_user_with_links_refreshes_updated_by_and_keeps_created_by() {
        let txn = test_txn().await;
        let creator_id = seed_actor(&txn).await;
        let updater_id = seed_actor(&txn).await;

        let created = create_user_in_tx(
            &txn,
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
        let updated = update_user_in_tx(
            &txn,
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
        let after_status = update_user(&txn, model, updater_id).await.unwrap();
        assert!(after_status);
        let reloaded = find_by_id(&txn, created.id).await.unwrap().unwrap();
        assert_eq!(reloaded.updated_by, updater_id);
    }

    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn soft_delete_user_clears_role_links_and_marks_deleted() {
        let txn = test_txn().await;
        let actor_id = seed_actor(&txn).await;
        let role = sys_role::ActiveModel {
            role_name: Set(unique_name("del_role")),
            role_key: Set(unique_name("del_role_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .unwrap();

        let created = create_user_in_tx(
            &txn,
            sys_user::ActiveModel {
                username: Set(unique_name("del_target")),
                password: Set("x".to_string()),
                nickname: Set("删除目标".to_string()),
                ..Default::default()
            },
            vec![role.id],
            actor_id,
        )
        .await
        .unwrap();

        // 绑定后确有关联
        let links = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        assert_eq!(links.len(), 1, "删除前应存在角色关联");

        soft_delete_user_in_tx(&txn, created.id).await.unwrap();

        assert!(
            find_by_id(&txn, created.id).await.unwrap().is_none(),
            "软删后活记录查询不可见"
        );
        let links_after = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        assert!(links_after.is_empty(), "角色关联应被物理清空");
    }

    /// 审计字段过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique_name("audit_page");
        let a = sys_user::ActiveModel {
            username: Set(format!("{kw}a")),
            password: Set("x".to_string()),
            nickname: Set("审计过滤".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let b = sys_user::ActiveModel {
            username: Set(format!("{kw}b")),
            password: Set("x".to_string()),
            nickname: Set("审计过滤".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let base = chrono::Local::now().naive_local();
        // update_many 盖不同的审计人与时间：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_user::Entity::update_many()
                .filter(sys_user::Column::Id.eq(row))
                .col_expr(sys_user::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_user::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_user::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_user::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let all = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(all.total, 2, "前置：keyword 应命中两行");
        let by_creator = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                created_by: Some(7),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_creator.total, 1, "created_by=7 应只命中 a");
        let by_updater = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                updated_by: Some(8),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_updater.total, 1, "updated_by=8 应只命中 b");
        let ghost = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                created_by: Some(9_999_999_999),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(ghost.total, 0, "不存在的创建人应过滤为空");
        let created_after = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                created_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(created_after.total, 1, "begin=base-7s 应只剩 b（晚于阈值）");
        let created_before = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                created_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(created_before.total, 1, "end=base-7s 应只剩 a（早于阈值）");
        let updated_after = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                updated_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            updated_after.total, 1,
            "updated_at 范围与 created_at 同机制"
        );
        let updated_before = find_page(
            &db,
            &UserFilter {
                keyword: Some(kw.clone()),
                updated_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(updated_before.total, 1);
    }
}
