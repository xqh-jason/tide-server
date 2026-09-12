use crate::entity::sys_user::Model;
use crate::entity::{sys_role, sys_user, sys_user_dept, sys_user_position, sys_user_role};
use crate::modules::user::dto::UserFilter;
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{Condition, ConnectionTrait, DatabaseTransaction, QueryOrder, QuerySelect};

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

    // 全量替换关联：先**无条件**清空旧关联（空数组即清空），再插入新关联。
    // 与 role 域 update 口径一致；「是否清空」由 service 的业务语义决定，
    // repo 不做该判断（回归：曾用 `if !role_ids.is_empty()` 包住清空，导致空数组无法清空）。
    sys_user_role::Entity::delete_many()
        .filter(sys_user_role::Column::UserId.eq(user.id))
        .exec(txn)
        .await?;

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
///
/// 返回软删是否命中一行（`false` = 目标不存在或已软删）；「用户不存在」的判定
/// 与报错由 service 层负责，repo 只做数据变更、不产出业务错误。
pub(crate) async fn soft_delete_user_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<bool> {
    // 物理清空角色关联（关系表硬删除约定）
    sys_user_role::Entity::delete_many()
        .filter(sys_user_role::Column::UserId.eq(id))
        .exec(txn)
        .await?;

    // 软删主表：窄写 deleted_at（updated_at 由 DB 侧 ON UPDATE 刷新）
    let result = sys_user::Entity::update_many()
        .filter(sys_user::Column::Id.eq(id))
        .filter(sys_user::Column::DeletedAt.is_null())
        .col_expr(
            sys_user::Column::DeletedAt,
            Expr::value(Some(chrono::Local::now().naive_local())),
        )
        .exec(txn)
        .await?;

    Ok(result.rows_affected > 0)
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

pub async fn find_role_ids_by_user_id(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Vec<u64>> {
    let models = sys_user_role::Entity::find()
        .filter(sys_user_role::Column::UserId.eq(user_id))
        .column(sys_user_role::Column::RoleId)
        .all(db)
        .await?;
    Ok(models
        .into_iter()
        .map(|model| model.role_id)
        .collect::<Vec<_>>())
}

pub async fn find_dept_links_by_user_id(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Vec<sys_user_dept::Model>> {
    let models = sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    Ok(models)
}

pub async fn find_dept_links_by_user_ids(
    db: &impl ConnectionTrait,
    user_ids: &[u64],
) -> anyhow::Result<Vec<sys_user_dept::Model>> {
    let models = sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::UserId.is_in(user_ids.iter().copied()))
        .all(db)
        .await?;
    Ok(models)
}

/// 事务内全量替换用户的部门关联：先清旧行，再插入新行（空数组即清空）。
///
/// 无旧行时**跳过 DELETE**（空集合短路）：MySQL RR 隔离级别下，未命中的
/// `DELETE ... WHERE user_id = ?` 会在主键索引上加间隙锁，与并发事务的插入意向锁
/// 互斥，可致 1213 死锁（测试并发场景已复现）。无行时删除本身也无意义。
pub(crate) async fn replace_user_depts_in_tx(
    txn: &DatabaseTransaction,
    user_id: u64,
    links: Vec<sys_user_dept::ActiveModel>,
) -> anyhow::Result<()> {
    let existing = sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::UserId.eq(user_id))
        .count(txn)
        .await?;

    if existing > 0 {
        // 物理清空部门关联（关系表硬删除约定）
        sys_user_dept::Entity::delete_many()
            .filter(sys_user_dept::Column::UserId.eq(user_id))
            .exec(txn)
            .await?;
    }

    // 插入新部门关联
    if !links.is_empty() {
        sys_user_dept::Entity::insert_many(links).exec(txn).await?;
    }

    Ok(())
}

pub async fn find_position_links_by_user_id(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Vec<sys_user_position::Model>> {
    let models = sys_user_position::Entity::find()
        .filter(sys_user_position::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    Ok(models)
}

pub async fn find_position_links_by_user_ids(
    db: &impl ConnectionTrait,
    user_ids: &[u64],
) -> anyhow::Result<Vec<sys_user_position::Model>> {
    let models = sys_user_position::Entity::find()
        .filter(sys_user_position::Column::UserId.is_in(user_ids.iter().copied()))
        .all(db)
        .await?;
    Ok(models)
}

/// 事务内全量替换用户的职位关联：先清旧行，再插入新行（空数组即清空）。
///
/// 无旧行时**跳过 DELETE**（空集合短路，同 `replace_user_depts_in_tx`）：
/// MySQL RR 隔离级别下，未命中的 `DELETE ... WHERE user_id = ?` 会在主键索引上
/// 加间隙锁，与并发事务的插入意向锁互斥，可致 1213 死锁。无行时删除本身也无意义。
pub(crate) async fn replace_user_positions_in_tx(
    txn: &DatabaseTransaction,
    user_id: u64,
    links: Vec<sys_user_position::ActiveModel>,
) -> anyhow::Result<()> {
    let existing = sys_user_position::Entity::find()
        .filter(sys_user_position::Column::UserId.eq(user_id))
        .count(txn)
        .await?;

    if existing > 0 {
        // 物理清空职位关联（关系表硬删除约定）
        sys_user_position::Entity::delete_many()
            .filter(sys_user_position::Column::UserId.eq(user_id))
            .exec(txn)
            .await?;
    }

    // 插入新职位关联
    if !links.is_empty() {
        sys_user_position::Entity::insert_many(links)
            .exec(txn)
            .await?;
    }

    Ok(())
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

    /// find_roles_by_user_id 只返回启用且未删的角色：禁用/软删角色不贡献
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
        assert!(!ids.contains(&disabled.id), "禁用角色应被过滤");
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

    /// 造一个操作人用户（直接 insert，不走 repo；其审计字段为 0（种子/系统写入口径）属预期）。
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
    // 全量替换语义回归：update 传空 role_ids 必须清空既有角色关联。
    // （曾因 repo 用 `if !role_ids.is_empty()` 包住清空逻辑，导致空数组无法清空关联。）
    async fn update_user_with_empty_role_ids_clears_role_links() {
        let txn = test_txn().await;
        let role = sys_role::ActiveModel {
            role_name: Set(unique_name("clear_role")),
            role_key: Set(unique_name("clear_role_key")),
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
                username: Set(unique_name("clear_links")),
                password: Set("x".to_string()),
                nickname: Set("清空关联用户".to_string()),
                ..Default::default()
            },
            vec![role.id],
            ACTOR_ID,
        )
        .await
        .unwrap();

        let before = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        assert_eq!(before.len(), 1, "前置：应已建立一条角色关联");

        // 全量替换传空数组 = 清空关联
        update_user_in_tx(
            &txn,
            sys_user::ActiveModel {
                id: Set(created.id),
                nickname: Set("清空关联用户v2".to_string()),
                ..Default::default()
            },
            vec![],
            ACTOR_ID,
        )
        .await
        .unwrap();

        let after = sys_user_role::Entity::find()
            .filter(sys_user_role::Column::UserId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        assert!(after.is_empty(), "空 role_ids 应清空既有角色关联");
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

        assert!(
            soft_delete_user_in_tx(&txn, created.id).await.unwrap(),
            "软删应命中一行"
        );

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

    /// 直接插入一个用户（部门关联测试不关心角色/权限），返回 id。
    async fn seed_user_for_depts(db: &impl ConnectionTrait) -> u64 {
        sys_user::ActiveModel {
            username: Set(unique_name("dept_link_user")),
            password: Set("x".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    /// 构造一条用户-部门关联（关系表无外键，dept_id 用任意值即可）。
    fn dept_link(
        user_id: u64,
        dept_id: u64,
        is_primary: i8,
        is_leader: i8,
    ) -> sys_user_dept::ActiveModel {
        sys_user_dept::ActiveModel {
            user_id: Set(user_id),
            dept_id: Set(dept_id),
            is_primary: Set(is_primary),
            is_leader: Set(is_leader),
        }
    }

    #[tokio::test]
    // 全量替换语义：先清旧关联再插入，旧行不残留。
    async fn replace_user_depts_clears_old_links_and_inserts_new() {
        let txn = test_txn().await;
        let user_id = seed_user_for_depts(&txn).await;

        replace_user_depts_in_tx(
            &txn,
            user_id,
            vec![dept_link(user_id, 11, 1, 0), dept_link(user_id, 12, 0, 1)],
        )
        .await
        .unwrap();
        let after_first = find_dept_links_by_user_id(&txn, user_id).await.unwrap();
        assert_eq!(after_first.len(), 2);

        replace_user_depts_in_tx(&txn, user_id, vec![dept_link(user_id, 13, 1, 0)])
            .await
            .unwrap();
        let after_second = find_dept_links_by_user_id(&txn, user_id).await.unwrap();
        assert_eq!(after_second.len(), 1, "旧关联应被清空");
        assert_eq!(after_second[0].dept_id, 13);
        assert_eq!(after_second[0].is_primary, 1);
    }

    #[tokio::test]
    // 空数组 = 清空全部关联。
    async fn replace_user_depts_with_empty_links_clears_all() {
        let txn = test_txn().await;
        let user_id = seed_user_for_depts(&txn).await;
        replace_user_depts_in_tx(
            &txn,
            user_id,
            vec![dept_link(user_id, 21, 1, 0), dept_link(user_id, 22, 0, 0)],
        )
        .await
        .unwrap();
        assert_eq!(
            find_dept_links_by_user_id(&txn, user_id)
                .await
                .unwrap()
                .len(),
            2
        );

        replace_user_depts_in_tx(&txn, user_id, vec![])
            .await
            .unwrap();
        assert!(
            find_dept_links_by_user_id(&txn, user_id)
                .await
                .unwrap()
                .is_empty(),
            "空数组应清空关联"
        );
    }

    #[tokio::test]
    // 批量查询：只返回指定用户的行；空入参短路返回空数组。
    async fn find_dept_links_by_user_ids_filters_by_user_and_short_circuits_empty() {
        let txn = test_txn().await;
        let user_a = seed_user_for_depts(&txn).await;
        let user_b = seed_user_for_depts(&txn).await;
        let user_c = seed_user_for_depts(&txn).await;
        replace_user_depts_in_tx(&txn, user_a, vec![dept_link(user_a, 31, 1, 0)])
            .await
            .unwrap();
        replace_user_depts_in_tx(
            &txn,
            user_b,
            vec![dept_link(user_b, 32, 1, 0), dept_link(user_b, 33, 0, 0)],
        )
        .await
        .unwrap();
        replace_user_depts_in_tx(&txn, user_c, vec![dept_link(user_c, 34, 1, 0)])
            .await
            .unwrap();

        let found = find_dept_links_by_user_ids(&txn, &[user_a, user_b])
            .await
            .unwrap();
        assert_eq!(found.len(), 3, "应只命中 a、b 两用户的关联");
        assert!(
            found
                .iter()
                .all(|link| link.user_id == user_a || link.user_id == user_b)
        );

        assert!(
            find_dept_links_by_user_ids(&txn, &[])
                .await
                .unwrap()
                .is_empty(),
            "空入参应短路返回空数组"
        );
    }

    #[tokio::test]
    // DB 约束兜底：同一用户至多一条 is_primary = 1（生成列 + 唯一索引），
    // 防绕过 validate 的写入路径（脚本 / 并发 / 未来新增写入方）产生多主部门。
    async fn user_dept_primary_unique_is_enforced_by_database() {
        let txn = test_txn().await;
        let user_id = seed_user_for_depts(&txn).await;

        dept_link(user_id, 41, 1, 0).insert(&txn).await.unwrap();

        let second_primary = dept_link(user_id, 42, 1, 0).insert(&txn).await;
        assert!(
            second_primary.is_err(),
            "第二行 is_primary=1 应被唯一约束拒绝: {second_primary:?}"
        );

        // 非主部门不受影响：同用户可写多行 is_primary=0
        dept_link(user_id, 43, 0, 0).insert(&txn).await.unwrap();
        dept_link(user_id, 44, 0, 0).insert(&txn).await.unwrap();
        assert_eq!(
            find_dept_links_by_user_id(&txn, user_id)
                .await
                .unwrap()
                .len(),
            3,
            "一行主部门 + 两行非主部门"
        );
    }

    /// 构造一条用户-职位关联（关系表无外键，position_id 用任意值即可）。
    fn position_link(user_id: u64, position_id: u64) -> sys_user_position::ActiveModel {
        sys_user_position::ActiveModel {
            user_id: Set(user_id),
            position_id: Set(position_id),
        }
    }

    #[tokio::test]
    // 全量替换语义：先清旧关联再插入，旧行不残留（兼任多职位可多行）。
    async fn replace_user_positions_clears_old_links_and_inserts_new() {
        let txn = test_txn().await;
        let user_id = seed_user_for_depts(&txn).await;

        replace_user_positions_in_tx(
            &txn,
            user_id,
            vec![position_link(user_id, 11), position_link(user_id, 12)],
        )
        .await
        .unwrap();
        let after_first = find_position_links_by_user_id(&txn, user_id).await.unwrap();
        assert_eq!(after_first.len(), 2);

        replace_user_positions_in_tx(&txn, user_id, vec![position_link(user_id, 13)])
            .await
            .unwrap();
        let after_second = find_position_links_by_user_id(&txn, user_id).await.unwrap();
        assert_eq!(after_second.len(), 1, "旧关联应被清空");
        assert_eq!(after_second[0].position_id, 13);
    }

    #[tokio::test]
    // 空数组 = 清空全部职位关联。
    async fn replace_user_positions_with_empty_links_clears_all() {
        let txn = test_txn().await;
        let user_id = seed_user_for_depts(&txn).await;
        replace_user_positions_in_tx(
            &txn,
            user_id,
            vec![position_link(user_id, 21), position_link(user_id, 22)],
        )
        .await
        .unwrap();
        assert_eq!(
            find_position_links_by_user_id(&txn, user_id)
                .await
                .unwrap()
                .len(),
            2
        );

        replace_user_positions_in_tx(&txn, user_id, vec![])
            .await
            .unwrap();
        assert!(
            find_position_links_by_user_id(&txn, user_id)
                .await
                .unwrap()
                .is_empty(),
            "空数组应清空关联"
        );
    }

    #[tokio::test]
    // 无旧行时跳过 DELETE（空集合短路）：防 RR 间隙锁死锁，也避免无意义删除。
    async fn replace_user_positions_without_old_rows_skips_delete_and_inserts() {
        let txn = test_txn().await;
        let user_id = seed_user_for_depts(&txn).await;

        // 全新用户直接插入：无旧行，不应触发 DELETE
        replace_user_positions_in_tx(&txn, user_id, vec![position_link(user_id, 31)])
            .await
            .unwrap();

        let links = find_position_links_by_user_id(&txn, user_id).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].position_id, 31);
    }

    #[tokio::test]
    // 批量查询：只返回指定用户的行；空入参短路返回空数组。
    async fn find_position_links_by_user_ids_filters_by_user_and_short_circuits_empty() {
        let txn = test_txn().await;
        let user_a = seed_user_for_depts(&txn).await;
        let user_b = seed_user_for_depts(&txn).await;
        replace_user_positions_in_tx(
            &txn,
            user_a,
            vec![position_link(user_a, 41), position_link(user_a, 42)],
        )
        .await
        .unwrap();
        replace_user_positions_in_tx(&txn, user_b, vec![position_link(user_b, 43)])
            .await
            .unwrap();

        let found = find_position_links_by_user_ids(&txn, &[user_a, user_b])
            .await
            .unwrap();
        assert_eq!(found.len(), 3, "应命中 a、b 两用户共三条关联");
        assert!(
            found
                .iter()
                .all(|link| link.user_id == user_a || link.user_id == user_b)
        );

        assert!(
            find_position_links_by_user_ids(&txn, &[])
                .await
                .unwrap()
                .is_empty(),
            "空入参应短路返回空数组"
        );
    }
}
