//! 员工档案数据访问原语（只拼 SQL：过滤 / 排序 / 分页 / 审计盖章 / 软删标记）。
//!
//! 写原语遵循 `*_in_tx` 模式：不自行 begin/commit，事务边界由 service 入口与
//! 测试外层事务负责；审计字段由 repo 统一盖章。查询原语按约定过滤软删，
//! 唯一例外是 `find_by_user_id_include_deleted`——`uk_hr_employee_user_id` 是单列
//! 唯一键，软删行仍占位，查重必须能查到软删记录（口径同 `sys_position.position_code`）。
//!
//! 业务判断（存在性文案、查重决策、字段是否可改）不在本层：
//! repo 只用 `Option` / `bool` 表达事实，文案与决策留给 service。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseTransaction, QueryOrder};

use crate::entity::hr_employee;
use crate::modules::biz::hr::employee::dto::EmployeeFilter;
use crate::utils::PageData;

/// 按 id 查有效档案（排除软删）。
pub async fn find_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_employee::Model>> {
    let employee = hr_employee::Entity::find()
        .filter(hr_employee::Column::Id.eq(id))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(employee)
}

/// 按 `user_id` 查档案——**含软删占位**（不过滤 `deleted_at`）。
///
/// 创建前查重专用：`user_id` 是单列唯一键，软删行仍占用，命中即拒绝新建
/// （「已存在员工档案」的判定与文案在 service 层）。
pub async fn find_by_user_id_include_deleted(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Option<hr_employee::Model>> {
    let employee = hr_employee::Entity::find()
        .filter(hr_employee::Column::UserId.eq(user_id))
        .one(db)
        .await?;
    Ok(employee)
}

/// 按员工 ID 批量取档案（用于额度列表 / 流水的员工名拼装）。
///
/// 软删过滤：`DeletedAt.is_null()`；不分页（调用方传已去重的 ID 集合）。
/// 空数组直接返回空 `Vec`——不发 `IN ()` 语句（那是 SQL 语法错误）。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> anyhow::Result<Vec<hr_employee::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let employees = hr_employee::Entity::find()
        .filter(hr_employee::Column::Id.is_in(ids.iter().copied()))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(employees)
}

/// 按 `user_id` 批量取员工档案（空入参早返；软删过滤：`DeletedAt.is_null()`）。
///
/// 供跨域按「账号」反查档案用（如按部门发放假期额度：部门 → 用户 → 档案）。
/// 不分页：调用方传已去重的 ID 集合。
pub async fn find_by_user_ids(
    db: &impl ConnectionTrait,
    user_ids: &[u64],
) -> anyhow::Result<Vec<hr_employee::Model>> {
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }
    let employees = hr_employee::Entity::find()
        .filter(hr_employee::Column::UserId.is_in(user_ids.iter().copied()))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(employees)
}

/// 分页 + 动态过滤：keyword 模糊备注 / 紧急联系人，状态与学历精确，
/// 审计人 / 时间范围过滤，按 id 降序（新档案在前），恒排除软删。
///
/// `page_index` 为 0-based、`page_size` 已由 `PageQuery` clamp 到 1..=1000。
pub async fn find_employee_page(
    db: &impl ConnectionTrait,
    filter: &EmployeeFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_employee::Model>> {
    let mut cond = Condition::all();

    if let Some(kw) = &filter.keyword {
        cond = cond.add(
            Condition::any()
                .add(hr_employee::Column::Remark.like(format!("%{kw}%")))
                .add(hr_employee::Column::EmergencyContact.like(format!("%{kw}%"))),
        );
    }

    if let Some(status) = filter.employment_status {
        cond = cond.add(hr_employee::Column::EmploymentStatus.eq(status));
    }

    if let Some(education) = filter.education {
        cond = cond.add(hr_employee::Column::Education.eq(education));
    }

    if let Some(created_by) = filter.created_by {
        cond = cond.add(hr_employee::Column::CreatedBy.eq(created_by));
    }

    if let Some(updated_by) = filter.updated_by {
        cond = cond.add(hr_employee::Column::UpdatedBy.eq(updated_by));
    }

    if let Some(created_at_begin) = filter.created_at_begin {
        cond = cond.add(hr_employee::Column::CreatedAt.gte(created_at_begin));
    }

    if let Some(created_at_end) = filter.created_at_end {
        cond = cond.add(hr_employee::Column::CreatedAt.lte(created_at_end));
    }

    if let Some(updated_at_begin) = filter.updated_at_begin {
        cond = cond.add(hr_employee::Column::UpdatedAt.gte(updated_at_begin));
    }

    if let Some(updated_at_end) = filter.updated_at_end {
        cond = cond.add(hr_employee::Column::UpdatedAt.lte(updated_at_end));
    }

    let select = hr_employee::Entity::find()
        .filter(cond)
        .filter(hr_employee::Column::DeletedAt.is_null())
        .order_by_desc(hr_employee::Column::Id);
    let page_data = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page_data)
}

/// 事务内创建档案：审计盖章（创建人与更新人同源，均取 `actor_id`）。
pub async fn create_employee_in_tx(
    txn: &DatabaseTransaction,
    model: hr_employee::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_employee::Model> {
    let model = hr_employee::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };

    let employee = model.insert(txn).await?;
    Ok(employee)
}

/// 事务内更新档案（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
///
/// 入参 `model` 由 service 构造：只 Set 业务变更列，其余列留 `NotSet`。
pub async fn update_employee_in_tx(
    txn: &DatabaseTransaction,
    model: hr_employee::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_employee::Model> {
    let model = hr_employee::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };

    let employee = model.update(txn).await?;
    Ok(employee)
}

/// 事务内软删档案：只更新 `deleted_at` + `updated_by`。
///
/// 返回是否有行被更新（`false` = 目标不存在或已软删）；
/// 「档案不存在」的判定与文案由 service 层负责。
pub async fn soft_delete_employee_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = hr_employee::Entity::update_many()
        .filter(hr_employee::Column::Id.eq(id))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .set(hr_employee::ActiveModel {
            deleted_at: Set(Some(chrono::Local::now().naive_local())),
            updated_by: Set(actor_id),
            ..Default::default()
        })
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;
    /// 档案归属账号的 id 基段：避开真实用户 id，且与 service 用例（900_2xx 段）不相交。
    const USER_ID_BASE: u64 = 900_100_000;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一 `user_id`：`uk_hr_employee_user_id` 是单列唯一键，同一进程内并行用例
    /// （repo / service 两个文件）必须用不同 id，否则撞唯一键。
    fn unique_user_id() -> u64 {
        USER_ID_BASE
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed)
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

    async fn seed_employee(
        db: &impl ConnectionTrait,
        user_id: u64,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> hr_employee::Model {
        hr_employee::ActiveModel {
            user_id: Set(user_id),
            employment_status: Set(1),
            education: Set(0),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted_while_include_deleted_helper_finds_it() {
        let db = test_txn().await;
        let live = seed_employee(&db, unique_user_id(), None).await;
        let deleted = seed_employee(&db, unique_user_id(), Some(now())).await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        assert_eq!(found_live.map(|m| m.id), Some(live.id));
        assert!(found_deleted.is_none(), "软删档案不应被 find_by_id 查到");
    }

    #[tokio::test]
    async fn find_by_user_id_include_deleted_finds_soft_deleted() {
        let db = test_txn().await;
        let user_id = unique_user_id();
        let live = seed_employee(&db, user_id, None).await;
        let deleted_user_id = unique_user_id();
        let deleted = seed_employee(&db, deleted_user_id, Some(now())).await;

        let found_live = find_by_user_id_include_deleted(&db, user_id).await.unwrap();
        let found_deleted = find_by_user_id_include_deleted(&db, deleted_user_id)
            .await
            .unwrap();
        let ghost = find_by_user_id_include_deleted(&db, 999_999_999)
            .await
            .unwrap();

        assert_eq!(found_live.map(|m| m.id), Some(live.id));
        assert_eq!(
            found_deleted.map(|m| m.id),
            Some(deleted.id),
            "user_id 唯一键含软删占位，软删档案必须能查到"
        );
        assert!(ghost.is_none());
    }

    #[tokio::test]
    async fn find_employee_page_filters_by_keyword_and_status_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("hrpage");
        // 命中备注，在职
        let by_remark = seed_employee(&db, unique_user_id(), None).await;
        hr_employee::ActiveModel {
            id: Set(by_remark.id),
            remark: Set(kw.clone()),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        // 命中紧急联系人，在职：只有 keyword 命中面覆盖它
        let by_contact = seed_employee(&db, unique_user_id(), None).await;
        hr_employee::ActiveModel {
            id: Set(by_contact.id),
            emergency_contact: Set(format!("联{kw}")),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        // 命中备注但离职：只有状态过滤能排除它
        let resigned = seed_employee(&db, unique_user_id(), None).await;
        hr_employee::ActiveModel {
            id: Set(resigned.id),
            remark: Set(kw.clone()),
            employment_status: Set(3),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();
        // 命中备注但已软删：keyword 与状态过滤都必须排除它
        let deleted = seed_employee(&db, unique_user_id(), Some(now())).await;
        hr_employee::ActiveModel {
            id: Set(deleted.id),
            remark: Set(kw.clone()),
            ..Default::default()
        }
        .update(&db)
        .await
        .unwrap();

        let all = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let enabled = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                employment_status: Some(1),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        let mut all_ids: Vec<u64> = all.items.iter().map(|m| m.id).collect();
        all_ids.sort_unstable();
        let mut expected = vec![by_remark.id, by_contact.id, resigned.id];
        expected.sort_unstable();
        assert_eq!(all.total, 3, "软删与不匹配记录应排除，keyword 双列命中");
        assert_eq!(all_ids, expected);

        let mut enabled_ids: Vec<u64> = enabled.items.iter().map(|m| m.id).collect();
        enabled_ids.sort_unstable();
        let mut expected_enabled = vec![by_remark.id, by_contact.id];
        expected_enabled.sort_unstable();
        assert_eq!(enabled.total, 2, "status=1 应排除离职与软删记录");
        assert_eq!(enabled_ids, expected_enabled);
    }

    #[tokio::test]
    async fn find_employee_page_filters_by_education_and_audit_columns() {
        let db = test_txn().await;
        let kw = unique("hraudit");
        let bachelor = seed_employee(&db, unique_user_id(), None).await;
        let master = seed_employee(&db, unique_user_id(), None).await;
        let base = now();
        for (row, education, by, offset) in [
            (bachelor.id, 3_i8, 7_i64, -10),
            (master.id, 4_i8, 8_i64, -5),
        ] {
            hr_employee::Entity::update_many()
                .filter(hr_employee::Column::Id.eq(row))
                .col_expr(
                    hr_employee::Column::Remark,
                    sea_orm::sea_query::Expr::value(kw.clone()),
                )
                .col_expr(
                    hr_employee::Column::Education,
                    sea_orm::sea_query::Expr::value(education),
                )
                .col_expr(
                    hr_employee::Column::CreatedBy,
                    sea_orm::sea_query::Expr::value(by),
                )
                .col_expr(
                    hr_employee::Column::UpdatedBy,
                    sea_orm::sea_query::Expr::value(by),
                )
                .col_expr(
                    hr_employee::Column::CreatedAt,
                    sea_orm::sea_query::Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    hr_employee::Column::UpdatedAt,
                    sea_orm::sea_query::Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let by_education = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                education: Some(4),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let by_creator = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                created_by: Some(7),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let ghost = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                updated_by: Some(9_999_999_999),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let created_after = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                created_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let created_before = find_employee_page(
            &db,
            &EmployeeFilter {
                keyword: Some(kw.clone()),
                created_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let updated_after = find_employee_page(
            &db,
            &EmployeeFilter {
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
            by_education.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![master.id],
            "education=4 应只命中硕士"
        );
        assert_eq!(
            by_creator.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![bachelor.id],
            "created_by=7 应只命中本科那条"
        );
        assert_eq!(ghost.total, 0, "不存在的更新人应过滤为空");
        assert_eq!(created_after.total, 1, "begin=base-7s 应只剩较晚的硕士行");
        assert_eq!(created_before.total, 1, "end=base-7s 应只剩较早的本科行");
        assert_eq!(
            updated_after.total, 1,
            "updated_at 范围与 created_at 同一套机制"
        );
    }

    #[tokio::test]
    async fn create_employee_in_tx_stamps_actor_as_creator_and_updater() {
        let db = test_txn().await;
        let actor_id = 42_u64;

        let created = create_employee_in_tx(
            &db,
            hr_employee::ActiveModel {
                user_id: Set(unique_user_id()),
                employment_status: Set(1),
                education: Set(0),
                ..Default::default()
            },
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id, "创建人应为操作人");
        assert_eq!(created.updated_by, actor_id, "创建时更新人与创建人同源");
        assert!(created.deleted_at.is_none(), "新建档案不应是软删态");
    }

    #[tokio::test]
    async fn update_employee_in_tx_refreshes_updated_by_and_keeps_created_by() {
        let db = test_txn().await;
        let creator_id = 41_u64;
        let updater_id = 42_u64;

        let created = create_employee_in_tx(
            &db,
            hr_employee::ActiveModel {
                user_id: Set(unique_user_id()),
                employment_status: Set(1),
                education: Set(0),
                remark: Set("盖章原始备注".to_string()),
                ..Default::default()
            },
            creator_id,
        )
        .await
        .unwrap();

        // 窄写：只 Set 变更列（备注），其余列 NotSet
        let updated = update_employee_in_tx(
            &db,
            hr_employee::ActiveModel {
                id: Set(created.id),
                remark: Set("盖章改名备注".to_string()),
                ..Default::default()
            },
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);
        assert_eq!(updated.remark, "盖章改名备注");
        assert_eq!(updated.user_id, created.user_id, "未 Set 的列不应被覆盖");
    }

    #[tokio::test]
    async fn soft_delete_employee_in_tx_returns_whether_row_hit() {
        let db = test_txn().await;
        let live = seed_employee(&db, unique_user_id(), None).await;
        let already = seed_employee(&db, unique_user_id(), Some(now())).await;

        let first = soft_delete_employee_in_tx(&db, live.id, ACTOR_ID)
            .await
            .unwrap();
        let again = soft_delete_employee_in_tx(&db, live.id, ACTOR_ID)
            .await
            .unwrap();
        let ghost = soft_delete_employee_in_tx(&db, 9_999_999_999, ACTOR_ID)
            .await
            .unwrap();
        let hit_deleted = soft_delete_employee_in_tx(&db, already.id, ACTOR_ID)
            .await
            .unwrap();

        assert!(first, "首次软删应命中");
        assert!(!again, "重复软删应返回 false（已软删）");
        assert!(!ghost, "不存在的 id 应返回 false");
        assert!(!hit_deleted, "已软删的行再删应返回 false");

        let after = find_by_id(&db, live.id).await.unwrap();
        assert!(after.is_none(), "软删后 find_by_id 不应再查到");
        let stamped = find_by_user_id_include_deleted(&db, live.user_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stamped.updated_by, ACTOR_ID, "软删应盖章更新人");
        assert!(stamped.deleted_at.is_some(), "软删应写入 deleted_at");
    }

    /// 批量取名入口：只回命中的有效档案（软删行不算命中），空入参不走 `IN ()`。
    #[tokio::test]
    async fn find_by_ids_returns_only_live_rows_for_requested_ids() {
        let db = test_txn().await;
        let first = seed_employee(&db, unique_user_id(), None).await;
        let second = seed_employee(&db, unique_user_id(), None).await;
        let deleted = seed_employee(&db, unique_user_id(), Some(now())).await;

        let empty = find_by_ids(&db, &[]).await.unwrap();
        assert!(empty.is_empty(), "空入参应直接返回空集合");

        let found = find_by_ids(&db, &[first.id, second.id, deleted.id])
            .await
            .unwrap();
        let mut ids: Vec<u64> = found.iter().map(|model| model.id).collect();
        ids.sort_unstable();
        let mut expected = [first.id, second.id];
        expected.sort_unstable();
        assert_eq!(ids, expected, "只应返回命中的有效档案（软删行不计入）");
    }
}
