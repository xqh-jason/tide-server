//! 考勤域数据访问原语（只拼 SQL：过滤 / 排序 / 分页 / 审计盖章 / 软删标记 / upsert）。
//!
//! 约定：
//! - `hr_shift` 是软删主表，逐查询 `.filter(DeletedAt.is_null())`；`hr_shift_schedule` /
//!   `hr_attendance_record` / `hr_work_calendar` **没有 `deleted_at` 列**（排班 / 事实 / 日历
//!   靠唯一键 upsert，不做软删——软删占位会让唯一键在重录时撞键，见 AGENTS.md 陷阱 11）；
//! - 写原语一律 `*_in_tx`：只收 `&DatabaseTransaction`，不自行 begin/commit，
//!   事务边界由 service 入口与测试外层事务负责；
//! - 审计字段由本层盖章：create 写 `created_by` + `updated_by`，update 只刷 `updated_by`，
//!   `created_by` 保持 `NotSet` 不被覆盖；
//! - 列表 / 详情一律普通读；本域没有「读 → 判断 → 写」的加锁读场景（排班与出勤事实的
//!   upsert 冲突由唯一键兜底，冲突即整事务回滚，不靠 `SELECT ... FOR UPDATE`）；
//! - 分页唯一执行器 `crate::utils::paginate`，分页查询必须带确定 `ORDER BY`；
//! - 业务判断（存在性文案、编码查重决策、外部 ID 冲突判定）不在本层。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseTransaction, QueryOrder};

use crate::entity::{hr_attendance_record, hr_shift, hr_shift_schedule, hr_work_calendar};
use crate::modules::biz::hr::attendance::dto::{
    CalendarFilter, RecordFilter, ScheduleFilter, ShiftFilter,
};
use crate::utils::PageData;

// —— 班次 `hr_shift`（软删主表）——

/// 分页 + 动态过滤班次（keyword 模糊编码 / 名称，status 精确），按 id 降序（新建在前），
/// 恒排除软删。
///
/// `page_index` 为 0-based、`page_size` 已由 `PageQuery` clamp 到 1..=1000。
pub async fn find_shift_page(
    db: &impl ConnectionTrait,
    filter: &ShiftFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_shift::Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = &filter.keyword {
        let like_keyword = format!("%{}%", keyword);
        cond = cond.add(
            Condition::any()
                .add(hr_shift::Column::ShiftCode.like(like_keyword.clone()))
                .add(hr_shift::Column::ShiftName.like(like_keyword)),
        );
    }

    if let Some(status) = filter.status {
        cond = cond.add(hr_shift::Column::Status.eq(status));
    }

    let select = hr_shift::Entity::find()
        .filter(cond)
        .filter(hr_shift::Column::DeletedAt.is_null())
        .order_by_desc(hr_shift::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查有效班次（排除软删）。
pub async fn find_shift_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_shift::Model>> {
    let model = hr_shift::Entity::find()
        .filter(hr_shift::Column::Id.eq(id))
        .filter(hr_shift::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 按 `shift_code` 查班次——**含软删占位**（不过滤 `deleted_at`）。
///
/// `uk_hr_shift_code` 是单列唯一键，软删行仍占位，查重必须能看到软删记录
/// （口径同 `sys_position.position_code` / `hr_time_off_type.type_code`）；
/// 「编码已存在」的判定与文案在 service 层。
pub async fn find_shift_by_code_include_deleted(
    db: &impl ConnectionTrait,
    code: &str,
) -> anyhow::Result<Option<hr_shift::Model>> {
    let model = hr_shift::Entity::find()
        .filter(hr_shift::Column::ShiftCode.eq(code))
        .one(db)
        .await?;
    Ok(model)
}

/// 按 ID 批量取有效班次（软删过滤；不分页；空入参直接返回空 `Vec`——不发 `IN ()`）。
pub async fn find_shifts_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> anyhow::Result<Vec<hr_shift::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = hr_shift::Entity::find()
        .filter(hr_shift::Column::Id.is_in(ids.iter().copied()))
        .filter(hr_shift::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(models)
}

/// 事务内创建班次：审计盖章（创建人与更新人同源，均取 `actor_id`）。
pub async fn create_shift_in_tx(
    txn: &DatabaseTransaction,
    model: hr_shift::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_shift::Model> {
    let active_model = hr_shift::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.insert(txn).await?;
    Ok(model)
}

/// 事务内更新班次（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
///
/// 入参 `model` 由 service 构造：只 Set 业务变更列，其余列留 `NotSet`。
pub async fn update_shift_in_tx(
    txn: &DatabaseTransaction,
    model: hr_shift::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_shift::Model> {
    let active_model = hr_shift::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.update(txn).await?;
    Ok(model)
}

/// 事务内软删班次：只更新 `deleted_at` + `updated_by`。
///
/// 返回是否有行被更新（`false` = 目标不存在或已软删）；「班次不存在」的判定与文案由
/// service 层负责。班次被历史排班引用**不阻止软删**（历史排班仍要能取名，故只标记不物理删）。
pub async fn soft_delete_shift_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let now = chrono::Local::now().naive_local();

    let result = hr_shift::Entity::update_many()
        .filter(hr_shift::Column::Id.eq(id))
        .filter(hr_shift::Column::DeletedAt.is_null())
        .set(hr_shift::ActiveModel {
            deleted_at: Set(Some(now)),
            updated_by: Set(actor_id),
            ..Default::default()
        })
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

// —— 排班 `hr_shift_schedule`（不软删，唯一键 `(employee_id, work_date)`）——

/// 分页 + 动态过滤排班（employee_id / shift_id / status 精确，日期区间闭区间），
/// 按 `work_date` 降序、`id` 降序（同日新建在前）。
pub async fn find_schedule_page(
    db: &impl ConnectionTrait,
    filter: &ScheduleFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_shift_schedule::Model>> {
    let mut cond = Condition::all();

    if let Some(employee_id) = filter.employee_id {
        cond = cond.add(hr_shift_schedule::Column::EmployeeId.eq(employee_id));
    }
    if let Some(shift_id) = filter.shift_id {
        cond = cond.add(hr_shift_schedule::Column::ShiftId.eq(shift_id));
    }
    if let Some(status) = filter.status {
        cond = cond.add(hr_shift_schedule::Column::Status.eq(status));
    }
    if let Some(begin) = filter.work_date_begin {
        cond = cond.add(hr_shift_schedule::Column::WorkDate.gte(begin));
    }
    if let Some(end) = filter.work_date_end {
        cond = cond.add(hr_shift_schedule::Column::WorkDate.lte(end));
    }

    let select = hr_shift_schedule::Entity::find()
        .filter(cond)
        .order_by_desc(hr_shift_schedule::Column::WorkDate)
        .order_by_desc(hr_shift_schedule::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查排班（详情用，不加锁）。
pub async fn find_schedule_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_shift_schedule::Model>> {
    let model = hr_shift_schedule::Entity::find()
        .filter(hr_shift_schedule::Column::Id.eq(id))
        .one(db)
        .await?;
    Ok(model)
}

/// 按唯一键 `(employee_id, work_date)` 查排班（upsert 的探测读；不加锁）。
pub async fn find_schedule_by_employee_date(
    db: &impl ConnectionTrait,
    employee_id: u64,
    work_date: Date,
) -> anyhow::Result<Option<hr_shift_schedule::Model>> {
    let model = hr_shift_schedule::Entity::find()
        .filter(hr_shift_schedule::Column::EmployeeId.eq(employee_id))
        .filter(hr_shift_schedule::Column::WorkDate.eq(work_date))
        .one(db)
        .await?;
    Ok(model)
}

/// 按员工集合 + 日期区间（闭区间）批量取排班（月视图用；空入参早返空）。
pub async fn find_schedules_by_employee_ids_and_range(
    db: &impl ConnectionTrait,
    employee_ids: &[u64],
    start: Date,
    end: Date,
) -> anyhow::Result<Vec<hr_shift_schedule::Model>> {
    if employee_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = hr_shift_schedule::Entity::find()
        .filter(hr_shift_schedule::Column::EmployeeId.is_in(employee_ids.iter().copied()))
        .filter(hr_shift_schedule::Column::WorkDate.gte(start))
        .filter(hr_shift_schedule::Column::WorkDate.lte(end))
        .order_by_asc(hr_shift_schedule::Column::WorkDate)
        .order_by_asc(hr_shift_schedule::Column::Id)
        .all(db)
        .await?;
    Ok(models)
}

/// 事务内创建排班：审计盖章（创建人与更新人同源，均取 `actor_id`）。
pub async fn create_schedule_in_tx(
    txn: &DatabaseTransaction,
    model: hr_shift_schedule::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_shift_schedule::Model> {
    let active_model = hr_shift_schedule::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.insert(txn).await?;
    Ok(model)
}

/// 事务内更新排班（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
pub async fn update_schedule_in_tx(
    txn: &DatabaseTransaction,
    model: hr_shift_schedule::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_shift_schedule::Model> {
    let active_model = hr_shift_schedule::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.update(txn).await?;
    Ok(model)
}

// —— 出勤事实 `hr_attendance_record`（不软删，唯一键 `(employee_id, work_date)` / `(source, external_id)`）——

/// 分页 + 动态过滤出勤事实（employee_id / source / miss_clock 精确，日期区间闭区间），
/// 按 `work_date` 降序、`id` 降序。
pub async fn find_record_page(
    db: &impl ConnectionTrait,
    filter: &RecordFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_attendance_record::Model>> {
    let mut cond = Condition::all();

    if let Some(employee_id) = filter.employee_id {
        cond = cond.add(hr_attendance_record::Column::EmployeeId.eq(employee_id));
    }
    if let Some(source) = filter.source {
        cond = cond.add(hr_attendance_record::Column::Source.eq(source));
    }
    if let Some(miss_clock) = filter.miss_clock {
        cond = cond.add(hr_attendance_record::Column::MissClock.eq(miss_clock));
    }
    if let Some(begin) = filter.work_date_begin {
        cond = cond.add(hr_attendance_record::Column::WorkDate.gte(begin));
    }
    if let Some(end) = filter.work_date_end {
        cond = cond.add(hr_attendance_record::Column::WorkDate.lte(end));
    }

    let select = hr_attendance_record::Entity::find()
        .filter(cond)
        .order_by_desc(hr_attendance_record::Column::WorkDate)
        .order_by_desc(hr_attendance_record::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查出勤事实（详情用，不加锁）。
pub async fn find_record_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_attendance_record::Model>> {
    let model = hr_attendance_record::Entity::find()
        .filter(hr_attendance_record::Column::Id.eq(id))
        .one(db)
        .await?;
    Ok(model)
}

/// 按唯一键 `(employee_id, work_date)` 查出勤事实（upsert 的探测读；不加锁）。
pub async fn find_record_by_employee_date(
    db: &impl ConnectionTrait,
    employee_id: u64,
    work_date: Date,
) -> anyhow::Result<Option<hr_attendance_record::Model>> {
    let model = hr_attendance_record::Entity::find()
        .filter(hr_attendance_record::Column::EmployeeId.eq(employee_id))
        .filter(hr_attendance_record::Column::WorkDate.eq(work_date))
        .one(db)
        .await?;
    Ok(model)
}

/// 按唯一键 `(source, external_id)` 查第三方记录（导入判重用；不加锁）。
///
/// `external_id` 为空串按「无外部 ID」处理（返回 `None`），避免误判。
pub async fn find_record_by_source_external_id(
    db: &impl ConnectionTrait,
    source: i8,
    external_id: &str,
) -> anyhow::Result<Option<hr_attendance_record::Model>> {
    if external_id.is_empty() {
        return Ok(None);
    }

    let model = hr_attendance_record::Entity::find()
        .filter(hr_attendance_record::Column::Source.eq(source))
        .filter(hr_attendance_record::Column::ExternalId.eq(external_id))
        .one(db)
        .await?;
    Ok(model)
}

/// 事务内创建出勤事实：审计盖章（创建人与更新人同源，均取 `actor_id`）。
pub async fn create_record_in_tx(
    txn: &DatabaseTransaction,
    model: hr_attendance_record::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_attendance_record::Model> {
    let active_model = hr_attendance_record::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.insert(txn).await?;
    Ok(model)
}

/// 事务内更新出勤事实（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
pub async fn update_record_in_tx(
    txn: &DatabaseTransaction,
    model: hr_attendance_record::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_attendance_record::Model> {
    let active_model = hr_attendance_record::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.update(txn).await?;
    Ok(model)
}

// —— 工作日历 `hr_work_calendar`（不软删，唯一键 `calendar_date`）——

/// 分页 + 动态过滤日历（is_workday / holiday_type 精确，日期区间闭区间），
/// 按 `calendar_date` 降序、`id` 降序。
pub async fn find_calendar_page(
    db: &impl ConnectionTrait,
    filter: &CalendarFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_work_calendar::Model>> {
    let mut cond = Condition::all();

    if let Some(is_workday) = filter.is_workday {
        cond = cond.add(hr_work_calendar::Column::IsWorkday.eq(is_workday));
    }
    if let Some(holiday_type) = filter.holiday_type {
        cond = cond.add(hr_work_calendar::Column::HolidayType.eq(holiday_type));
    }
    if let Some(begin) = filter.date_begin {
        cond = cond.add(hr_work_calendar::Column::CalendarDate.gte(begin));
    }
    if let Some(end) = filter.date_end {
        cond = cond.add(hr_work_calendar::Column::CalendarDate.lte(end));
    }

    let select = hr_work_calendar::Entity::find()
        .filter(cond)
        .order_by_desc(hr_work_calendar::Column::CalendarDate)
        .order_by_desc(hr_work_calendar::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按唯一键 `calendar_date` 查日历（upsert 的探测读；不加锁）。
pub async fn find_calendar_by_date(
    db: &impl ConnectionTrait,
    calendar_date: Date,
) -> anyhow::Result<Option<hr_work_calendar::Model>> {
    let model = hr_work_calendar::Entity::find()
        .filter(hr_work_calendar::Column::CalendarDate.eq(calendar_date))
        .one(db)
        .await?;
    Ok(model)
}

/// 事务内创建日历行：审计盖章（创建人与更新人同源，均取 `actor_id`）。
pub async fn create_calendar_in_tx(
    txn: &DatabaseTransaction,
    model: hr_work_calendar::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_work_calendar::Model> {
    let active_model = hr_work_calendar::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.insert(txn).await?;
    Ok(model)
}

/// 事务内更新日历行（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
pub async fn update_calendar_in_tx(
    txn: &DatabaseTransaction,
    model: hr_work_calendar::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_work_calendar::Model> {
    let active_model = hr_work_calendar::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.update(txn).await?;
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::hr_employee;
    use crate::modules::biz::hr::attendance::{
        HOLIDAY_TYPE_STATUTORY, SCHEDULE_STATUS_NORMAL, SCHEDULE_STATUS_SWAPPED, SOURCE_DINGTALK,
        SOURCE_MANUAL, repo,
    };
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同进程内并行用例（repo / service 两个模块）必须互不相同，
    /// 否则撞 `uk_hr_shift_code`。
    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一员工 ID：段位 900_4xx，与 time_off（900_2xx / 900_3xx）、employee（900_1xx）
    /// 测试段位错开，避免同进程并行撞 `uk_hr_employee_user_id`。
    fn unique_employee_id() -> u64 {
        900_400_000
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed) % 1_000
    }

    /// 唯一日历日期：基年 2098（service 测试用 2097），远离真实业务日期；
    /// `calendar_date` 是全局唯一键，并发用例必须各占一天。
    fn unique_calendar_date() -> Date {
        let offset = SEQ.fetch_add(1, Ordering::Relaxed) % 360;
        chrono::NaiveDate::from_ymd_opt(2098, 1, 1)
            .unwrap()
            .checked_add_days(chrono::Days::new(offset))
            .unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> Date {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn time(h: u32, m: u32, s: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, s).unwrap()
    }

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 测试用班次 ActiveModel（`shift_code` 必须唯一，其余取最小合法值）。
    fn shift_model(shift_code: String) -> hr_shift::ActiveModel {
        hr_shift::ActiveModel {
            shift_code: Set(shift_code),
            shift_name: Set("测试班次".to_owned()),
            start_time: Set(time(9, 0, 0)),
            end_time: Set(time(18, 0, 0)),
            cross_day: Set(0),
            work_minutes: Set(480),
            rest_minutes: Set(60),
            late_tolerance_minutes: Set(5),
            need_clock: Set(1),
            status: Set(1),
            ..Default::default()
        }
    }

    /// 直插一份员工档案，返回 `hr_employee.id`（非空列都有 DDL 默认值，只需 `user_id`）。
    async fn seed_employee(txn: &DatabaseTransaction) -> u64 {
        let employee = hr_employee::ActiveModel {
            user_id: Set(unique_employee_id() + 10_000),
            employment_status: Set(1),
            education: Set(0),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap();
        employee.id
    }

    #[tokio::test]
    async fn soft_deleted_shift_is_hidden_from_find_by_id_but_still_occupies_its_code() {
        let txn = test_txn().await;
        let code = unique("att_repo");
        let shift = repo::create_shift_in_tx(&txn, shift_model(code.clone()), ACTOR_ID)
            .await
            .unwrap();

        assert!(
            repo::soft_delete_shift_in_tx(&txn, shift.id, ACTOR_ID)
                .await
                .unwrap(),
            "首次软删必须返回 true"
        );
        assert!(
            !repo::soft_delete_shift_in_tx(&txn, shift.id, ACTOR_ID)
                .await
                .unwrap(),
            "重复软删必须返回 false（已软删行不再命中）"
        );
        assert!(
            repo::find_shift_by_id(&txn, shift.id)
                .await
                .unwrap()
                .is_none(),
            "软删后 find_by_id 必须查不到"
        );
        assert!(
            repo::find_shifts_by_ids(&txn, &[shift.id])
                .await
                .unwrap()
                .is_empty(),
            "软删后批量取名必须取不到"
        );
        // 单列唯一键仍占位：查重必须看得见软删行，否则重建会同码撞键
        assert!(
            repo::find_shift_by_code_include_deleted(&txn, &code)
                .await
                .unwrap()
                .is_some(),
            "软删班次必须仍占用班次编码"
        );
    }

    #[tokio::test]
    async fn find_shift_page_filters_keyword_and_status_and_hides_deleted() {
        let txn = test_txn().await;
        let kw = unique("att_shiftpage");

        // 命中 shift_code，启用
        let mut by_code = shift_model(format!("{kw}_code"));
        by_code.shift_name = Set("与此无关的名称".to_owned());
        let by_code = repo::create_shift_in_tx(&txn, by_code, ACTOR_ID)
            .await
            .unwrap();

        // 只命中 shift_name，启用
        let mut by_name = shift_model(unique("att_shiftpageother"));
        by_name.shift_name = Set(format!("班次{kw}"));
        let by_name = repo::create_shift_in_tx(&txn, by_name, ACTOR_ID)
            .await
            .unwrap();

        // 命中 keyword 但已停用：只有 status 过滤能排除它
        let mut disabled = shift_model(format!("{kw}_off"));
        disabled.status = Set(0);
        let disabled = repo::create_shift_in_tx(&txn, disabled, ACTOR_ID)
            .await
            .unwrap();

        // 命中 keyword 但已软删：keyword 与 soft-delete 过滤都必须排除它
        let deleted = repo::create_shift_in_tx(&txn, shift_model(format!("{kw}_del")), ACTOR_ID)
            .await
            .unwrap();
        repo::soft_delete_shift_in_tx(&txn, deleted.id, ACTOR_ID)
            .await
            .unwrap();

        let all = repo::find_shift_page(
            &txn,
            &ShiftFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let enabled = repo::find_shift_page(
            &txn,
            &ShiftFilter {
                keyword: Some(kw.clone()),
                status: Some(1),
            },
            0,
            10,
        )
        .await
        .unwrap();

        let mut all_ids: Vec<u64> = all.items.iter().map(|m| m.id).collect();
        all_ids.sort_unstable();
        let mut expected = vec![by_code.id, by_name.id, disabled.id];
        expected.sort_unstable();
        assert_eq!(all.total, 3, "keyword 应双列命中，且排除软删记录");
        assert_eq!(all_ids, expected);

        let mut enabled_ids: Vec<u64> = enabled.items.iter().map(|m| m.id).collect();
        enabled_ids.sort_unstable();
        let mut expected_enabled = vec![by_code.id, by_name.id];
        expected_enabled.sort_unstable();
        assert_eq!(enabled.total, 2, "status=1 应排除停用记录");
        assert_eq!(enabled_ids, expected_enabled);
    }

    #[tokio::test]
    async fn schedule_upsert_primitives_are_keyed_by_employee_and_date() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let work_date = date(2026, 9, 7);

        assert!(
            repo::find_schedule_by_employee_date(&txn, employee_id, work_date)
                .await
                .unwrap()
                .is_none(),
            "未排班时必须返回 None"
        );

        let created = repo::create_schedule_in_tx(
            &txn,
            hr_shift_schedule::ActiveModel {
                employee_id: Set(employee_id),
                work_date: Set(work_date),
                shift_id: Set(0),
                status: Set(SCHEDULE_STATUS_NORMAL),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();
        assert_eq!(created.created_by, ACTOR_ID, "创建人必须由 repo 盖章");
        assert_eq!(created.updated_by, ACTOR_ID, "创建时更新人必须与创建人同源");

        let found = repo::find_schedule_by_employee_date(&txn, employee_id, work_date)
            .await
            .unwrap()
            .expect("建行后必须能按 (employee_id, work_date) 查到");
        assert_eq!(found.id, created.id);
        assert_eq!(found.shift_id, 0, "0 = 当天休息必须原样落库");

        // 给同一天换个班次并标「已换班」：upsert 走 update，不新建第二行
        let other_actor = ACTOR_ID + 7;
        let updated = repo::update_schedule_in_tx(
            &txn,
            hr_shift_schedule::ActiveModel {
                id: Set(created.id),
                shift_id: Set(42),
                status: Set(SCHEDULE_STATUS_SWAPPED),
                ..Default::default()
            },
            other_actor,
        )
        .await
        .unwrap();
        assert_eq!(updated.created_by, ACTOR_ID, "窄写更新不得覆盖创建人");
        assert_eq!(updated.updated_by, other_actor, "更新必须刷新更新人");
        assert_eq!(updated.shift_id, 42, "Set 的列必须写入");
        assert_eq!(updated.status, SCHEDULE_STATUS_SWAPPED);
        assert_eq!(updated.employee_id, employee_id, "未 Set 的列必须保持原值");
    }

    #[tokio::test]
    async fn record_external_id_lookup_is_scoped_by_source() {
        let txn = test_txn().await;
        let employee_id = seed_employee(&txn).await;
        let external_id = unique("att_ext");

        let created = repo::create_record_in_tx(
            &txn,
            hr_attendance_record::ActiveModel {
                employee_id: Set(employee_id),
                work_date: Set(date(2026, 9, 8)),
                source: Set(SOURCE_DINGTALK),
                external_id: Set(Some(external_id.clone())),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();

        let hit = repo::find_record_by_source_external_id(&txn, SOURCE_DINGTALK, &external_id)
            .await
            .unwrap()
            .expect("同 source + external_id 必须命中");
        assert_eq!(hit.id, created.id);
        assert!(
            repo::find_record_by_source_external_id(&txn, SOURCE_MANUAL, &external_id)
                .await
                .unwrap()
                .is_none(),
            "不同 source 不得命中同一外部 ID"
        );
        assert!(
            repo::find_record_by_source_external_id(&txn, SOURCE_DINGTALK, "")
                .await
                .unwrap()
                .is_none(),
            "空外部 ID 必须按无外部 ID 处理"
        );

        let by_date = repo::find_record_by_employee_date(&txn, employee_id, date(2026, 9, 8))
            .await
            .unwrap()
            .expect("事实必须能按 (employee_id, work_date) 查到");
        assert_eq!(by_date.id, created.id);
        assert_eq!(
            by_date.external_id.as_deref(),
            Some(external_id.as_str()),
            "外部 ID 必须原样落库"
        );
    }

    #[tokio::test]
    async fn calendar_lookup_is_keyed_by_date_and_update_keeps_created_by() {
        let txn = test_txn().await;
        let calendar_date = unique_calendar_date();

        assert!(
            repo::find_calendar_by_date(&txn, calendar_date)
                .await
                .unwrap()
                .is_none(),
            "未配置时日历必须返回 None"
        );

        let created = repo::create_calendar_in_tx(
            &txn,
            hr_work_calendar::ActiveModel {
                calendar_date: Set(calendar_date),
                is_workday: Set(0),
                holiday_type: Set(HOLIDAY_TYPE_STATUTORY),
                standard_minutes: Set(0),
                remark: Set("国庆节".to_owned()),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();

        let other_actor = ACTOR_ID + 3;
        let updated = repo::update_calendar_in_tx(
            &txn,
            hr_work_calendar::ActiveModel {
                id: Set(created.id),
                is_workday: Set(1),
                holiday_type: Set(0),
                standard_minutes: Set(480),
                ..Default::default()
            },
            other_actor,
        )
        .await
        .unwrap();
        assert_eq!(updated.created_by, ACTOR_ID, "窄写更新不得覆盖创建人");
        assert_eq!(updated.updated_by, other_actor, "更新必须刷新更新人");
        assert_eq!(updated.is_workday, 1, "Set 的列必须写入");
        assert_eq!(updated.remark, "国庆节", "未 Set 的备注必须保持原值");

        let found = repo::find_calendar_by_date(&txn, calendar_date)
            .await
            .unwrap()
            .expect("按日期必须能查到日历行");
        assert_eq!(found.id, created.id);
    }

    #[tokio::test]
    async fn schedule_batch_range_lookup_returns_empty_for_empty_employee_ids() {
        let txn = test_txn().await;
        let models = repo::find_schedules_by_employee_ids_and_range(
            &txn,
            &[],
            date(2026, 9, 1),
            date(2026, 9, 30),
        )
        .await
        .unwrap();
        assert!(models.is_empty(), "空员工集合必须早返空，不发 IN ()");
    }
}
