//! 职位数据访问原语。
//!
//! 写原语遵循 `*_in_tx` 模式：不自行 begin/commit，边界由 service 入口与
//! 测试外层事务负责；审计字段由 repo 统一盖章。查询原语按约定过滤软删。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{Condition, ConnectionTrait, DatabaseTransaction, QueryOrder};

use crate::entity::{sys_position, sys_user_position};
use crate::modules::position::dto::PositionFilter;

/// 按 id 查询有效职位（排除软删；停用职位仍返回）。
pub async fn find_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<sys_position::Model>> {
    let model = sys_position::Entity::find()
        .filter(sys_position::Column::Id.eq(id))
        .filter(sys_position::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 批量按 id 查有效职位（排除软删；停用职位仍返回）。空入参短路，不产生空 `IN`。
///
/// 供跨域引用校验（如 user 挂载职位的存在性）与批量名称拼装使用。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> anyhow::Result<Vec<sys_position::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let models = sys_position::Entity::find()
        .filter(sys_position::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_position::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(models)
}

/// 查重辅助——position_code 唯一（含软删占位，不过滤 deleted_at）。
pub async fn find_by_code_include_deleted(
    db: &impl ConnectionTrait,
    position_code: &str,
) -> anyhow::Result<Option<sys_position::Model>> {
    let model = sys_position::Entity::find()
        .filter(sys_position::Column::PositionCode.eq(position_code))
        .one(db)
        .await?;
    Ok(model)
}

/// 分页 + 动态过滤（keyword 对编码 / 名称模糊，status 精确，
/// created_by/updated_by/时间范围审计过滤），sort 升序、id 升序。
pub async fn find_position_page(
    db: &impl ConnectionTrait,
    filter: &PositionFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<sys_position::Model>> {
    let mut cond = Condition::all();

    if let Some(v) = &filter.status {
        cond = cond.add(sys_position::Column::Status.eq(*v));
    }
    if let Some(v) = &filter.keyword {
        let kw_cond = Condition::any()
            .add(sys_position::Column::PositionCode.like(format!("%{v}%")))
            .add(sys_position::Column::PositionName.like(format!("%{v}%")));
        cond = cond.add(kw_cond);
    }

    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_position::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_position::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_position::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_position::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_position::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_position::Column::UpdatedAt.lte(v));
    }

    let select = sys_position::Entity::find()
        .filter(cond)
        .filter(sys_position::Column::DeletedAt.is_null())
        .order_by_asc(sys_position::Column::Sort)
        .order_by_asc(sys_position::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 事务内创建职位。`actor_id` 为操作人，审计字段由 repo 统一盖章（创建时创建人与更新人同源）。
pub async fn create_position_in_tx(
    txn: &DatabaseTransaction,
    model: sys_position::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<sys_position::Model> {
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    Ok(model.insert(txn).await?)
}

/// 事务内更新职位（主键必须已设置）。审计字段由 repo 统一盖章：只刷新更新人。
///
/// 入参 `model` 由调用方构造：只 Set 业务变更列，其余列留 `NotSet`，避免全列覆盖写。
pub async fn update_position_in_tx(
    txn: &DatabaseTransaction,
    model: sys_position::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<sys_position::Model> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    Ok(model.update(txn).await?)
}

/// 事务内软删职位（引用约束由 service 层负责）：只更新 `deleted_at` + `updated_by`。
///
/// 返回是否有行被更新（`false` = 目标不存在或已软删）；不产出业务错误文案，
/// 「职位不存在」的判定与报错由 service 层负责。
pub async fn soft_delete_position_in_tx(
    txn: &DatabaseTransaction,
    position_id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = sys_position::Entity::update_many()
        .filter(sys_position::Column::Id.eq(position_id))
        .filter(sys_position::Column::DeletedAt.is_null())
        .col_expr(
            sys_position::Column::DeletedAt,
            Expr::value(Some(chrono::Local::now().naive_local())),
        )
        .col_expr(sys_position::Column::UpdatedBy, Expr::value(actor_id))
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

/// 统计某职位被 `sys_user_position` 挂载的引用数（关系表硬删，不加软删过滤）。
///
/// 删除职位前的占用检查用；存在性 / 文案判定由 service 层负责。
pub async fn count_user_refs_by_position_id(
    db: &impl ConnectionTrait,
    position_id: u64,
) -> anyhow::Result<u64> {
    let count = sys_user_position::Entity::find()
        .filter(sys_user_position::Column::PositionId.eq(position_id))
        .count(db)
        .await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一命名：`<prefix>_<pid>_<seq>`，避免撞真实库唯一键。
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

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    fn now() -> chrono::NaiveDateTime {
        chrono::Local::now().naive_local()
    }

    async fn seed_position(
        db: &impl ConnectionTrait,
        code: &str,
        name: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_position::Model {
        sys_position::ActiveModel {
            position_code: Set(code.to_string()),
            position_name: Set(name.to_string()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted_but_keeps_disabled() {
        let db = test_txn().await;
        let live = seed_position(&db, &unique("live"), "在任", 1, None).await;
        let disabled = seed_position(&db, &unique("off"), "停用", 0, None).await;
        let deleted = seed_position(&db, &unique("del"), "已删", 1, Some(now())).await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_disabled = find_by_id(&db, disabled.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        assert_eq!(found_live.as_ref().map(|m| m.id), Some(live.id));
        assert!(found_disabled.is_some(), "停用职位仍是有效数据，应可查到");
        assert!(found_deleted.is_none(), "软删职位不应被 find_by_id 查到");
    }

    #[tokio::test]
    async fn find_by_ids_short_circuits_empty_input_and_excludes_soft_deleted() {
        let db = test_txn().await;
        let live = seed_position(&db, &unique("live"), "在任", 1, None).await;
        let deleted = seed_position(&db, &unique("del"), "已删", 1, Some(now())).await;

        let empty = find_by_ids(&db, &[]).await.unwrap();
        let mixed = find_by_ids(&db, &[live.id, deleted.id]).await.unwrap();

        assert!(empty.is_empty(), "空入参应短路返回空数组");
        assert_eq!(mixed.len(), 1, "软删职位不应出现在批量结果中");
        assert_eq!(mixed[0].id, live.id);
    }

    #[tokio::test]
    async fn find_by_code_include_deleted_finds_soft_deleted() {
        let db = test_txn().await;
        let live = seed_position(&db, &unique("code_live"), "在任", 1, None).await;
        let deleted = seed_position(&db, &unique("code_del"), "已删", 1, Some(now())).await;

        let found_live = find_by_code_include_deleted(&db, &live.position_code)
            .await
            .unwrap();
        let found_deleted = find_by_code_include_deleted(&db, &deleted.position_code)
            .await
            .unwrap();

        assert_eq!(found_live.map(|m| m.id), Some(live.id));
        assert_eq!(
            found_deleted.map(|m| m.id),
            Some(deleted.id),
            "编码唯一键含软删占位，软删记录必须能查到"
        );
    }

    #[tokio::test]
    async fn find_position_page_filters_by_keyword_and_status_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("kw");
        // 命中编码，启用
        let by_code = seed_position(&db, &format!("c{kw}"), &unique("nm"), 1, None).await;
        // 命中名称，启用
        let by_name = seed_position(&db, &unique("cd"), &format!("职{kw}"), 1, None).await;
        // 命中编码但停用：只有 status 过滤能排除它
        let disabled = seed_position(&db, &format!("c{kw}d"), &unique("nm"), 0, None).await;
        // 与 kw 无关，启用：只有 keyword 过滤能排除它
        let _unrelated = seed_position(&db, &unique("cd2"), &unique("nm2"), 1, None).await;
        // 命中编码但已软删：keyword / status 都必须排除它
        let _deleted = seed_position(&db, &format!("c{kw}x"), &unique("nm"), 1, Some(now())).await;

        let all = find_position_page(
            &db,
            &PositionFilter {
                keyword: Some(kw.clone()),
                status: None,
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let enabled = find_position_page(
            &db,
            &PositionFilter {
                keyword: Some(kw.clone()),
                status: Some(1),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        let all_ids: Vec<u64> = all.items.iter().map(|m| m.id).collect();
        assert_eq!(
            all.total, 3,
            "keyword 应同时命中编码与名称，软删与无关记录排除"
        );
        assert!(
            all_ids.contains(&by_code.id)
                && all_ids.contains(&by_name.id)
                && all_ids.contains(&disabled.id),
            "keyword 命中集应含启用与停用记录"
        );
        let enabled_ids: Vec<u64> = enabled.items.iter().map(|m| m.id).collect();
        assert_eq!(
            enabled_ids,
            vec![by_code.id.min(by_name.id), by_code.id.max(by_name.id)],
            "status=1 应排除停用与软删记录，且按 sort、id 升序"
        );
    }

    /// 审计字段过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_position_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique("audit_page");
        let a = seed_position(&db, &format!("c{kw}a"), &unique("nm"), 1, None).await;
        let b = seed_position(&db, &format!("c{kw}b"), &unique("nm"), 1, None).await;

        let base = chrono::Local::now().naive_local();
        // update_many 盖不同的审计人与时间：避免为测试改 seed 夹具
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_position::Entity::update_many()
                .filter(sys_position::Column::Id.eq(row))
                .col_expr(sys_position::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_position::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_position::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_position::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let all = find_position_page(
            &db,
            &PositionFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(all.total, 2, "前置：keyword 应命中两行");
        let by_creator = find_position_page(
            &db,
            &PositionFilter {
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
        let by_updater = find_position_page(
            &db,
            &PositionFilter {
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
        let ghost = find_position_page(
            &db,
            &PositionFilter {
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
        let created_after = find_position_page(
            &db,
            &PositionFilter {
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
        let created_before = find_position_page(
            &db,
            &PositionFilter {
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
        let updated_after = find_position_page(
            &db,
            &PositionFilter {
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
        let updated_before = find_position_page(
            &db,
            &PositionFilter {
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

    #[tokio::test]
    async fn create_position_in_tx_stamps_actor_as_creator_and_updater() {
        let db = test_txn().await;
        let actor_id = 42_u64;

        let created = create_position_in_tx(
            &db,
            sys_position::ActiveModel {
                position_code: Set(unique("stamp_c")),
                position_name: Set("盖章创建".to_string()),
                ..Default::default()
            },
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id);
        assert_eq!(created.updated_by, actor_id);
    }

    #[tokio::test]
    async fn update_position_in_tx_refreshes_updated_by_and_keeps_created_by() {
        let db = test_txn().await;
        let creator_id = 41_u64;
        let updater_id = 42_u64;

        let created = create_position_in_tx(
            &db,
            sys_position::ActiveModel {
                position_code: Set(unique("stamp_u")),
                position_name: Set("盖章更新".to_string()),
                ..Default::default()
            },
            creator_id,
        )
        .await
        .unwrap();

        // 窄写：只 Set 变更列（名称），其余列 NotSet
        let updated = update_position_in_tx(
            &db,
            sys_position::ActiveModel {
                id: Set(created.id),
                position_name: Set("盖章更新改名".to_string()),
                ..Default::default()
            },
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);
        assert_eq!(updated.position_name, "盖章更新改名");
        assert_eq!(
            updated.position_code, created.position_code,
            "未 Set 的列不应被覆盖"
        );
    }

    #[tokio::test]
    async fn soft_delete_position_in_tx_returns_whether_row_hit() {
        let db = test_txn().await;
        let live = seed_position(&db, &unique("sd_live"), "在任", 1, None).await;
        let deleted = seed_position(&db, &unique("sd_del"), "已删", 1, Some(now())).await;

        let first = soft_delete_position_in_tx(&db, live.id, ACTOR_ID)
            .await
            .unwrap();
        let again = soft_delete_position_in_tx(&db, live.id, ACTOR_ID)
            .await
            .unwrap();
        let ghost = soft_delete_position_in_tx(&db, 9_999_999_999, ACTOR_ID)
            .await
            .unwrap();
        let already = soft_delete_position_in_tx(&db, deleted.id, ACTOR_ID)
            .await
            .unwrap();

        assert!(first, "首次软删应命中");
        assert!(!again, "重复软删应返回 false（已软删）");
        assert!(!ghost, "不存在的 id 应返回 false");
        assert!(!already, "已软删的行再删应返回 false");

        let after = find_by_id(&db, live.id).await.unwrap();
        assert!(after.is_none(), "软删后 find_by_id 不应再查到");
        assert_eq!(
            find_by_code_include_deleted(&db, &live.position_code)
                .await
                .unwrap()
                .map(|m| m.deleted_at.is_some()),
            Some(true),
            "软删行仍占用编码（含软删占位口径）"
        );
    }

    #[tokio::test]
    async fn count_user_refs_by_position_id_counts_links() {
        let db = test_txn().await;
        let p = seed_position(&db, &unique("ref"), "被引用", 1, None).await;
        let other = seed_position(&db, &unique("ref2"), "未引用", 1, None).await;

        for user_id in [101_u64, 102_u64] {
            sys_user_position::ActiveModel {
                user_id: Set(user_id),
                position_id: Set(p.id),
            }
            .insert(&db)
            .await
            .unwrap();
        }

        let refs = count_user_refs_by_position_id(&db, p.id).await.unwrap();
        let none = count_user_refs_by_position_id(&db, other.id).await.unwrap();

        assert_eq!(refs, 2, "应统计全部挂载行");
        assert_eq!(none, 0, "无引用应为 0");
    }
}
