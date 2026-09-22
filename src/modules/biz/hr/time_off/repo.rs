//! 假期额度域数据访问原语（只拼 SQL：过滤 / 排序 / 分页 / 审计盖章 / 软删标记）。
//!
//! 约定：
//! - `hr_time_off_type` 是业务主表（软删），逐查询 `.filter(DeletedAt.is_null())`；
//!   账本三表（grant / balance / log）**没有 `deleted_at` 列**，作废走 `status` + 反向流水；
//! - 写原语一律 `*_in_tx`：只收 `&DatabaseTransaction`，不自行 begin/commit，
//!   事务边界由 service 入口与测试外层事务负责；
//! - 审计字段由本层盖章：create 写 `created_by` + `updated_by`，update 只刷 `updated_by`，
//!   `created_by` 保持 `NotSet` 不被覆盖；
//! - 「读 → 判断 → 写」才加行锁（`find_*_for_update`），列表 / 详情不加锁；
//! - 分页唯一执行器 `crate::utils::paginate`，分页查询必须带确定 `ORDER BY`；
//! - 业务判断（存在性文案、额度是否足够、幂等决策）不在本层：
//!   只用 `Option` / `bool` / `u64` / `PageData` 表达事实，文案与决策留给 service。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseTransaction, QueryOrder, QuerySelect};

use crate::entity::{
    hr_time_off_balance, hr_time_off_balance_log, hr_time_off_grant, hr_time_off_type,
};
use crate::modules::biz::hr::time_off::dto::{
    TimeOffBalanceFilter, TimeOffBalanceLogFilter, TimeOffGrantFilter, TimeOffTypeFilter,
};
use crate::utils::PageData;

/// 分页 + 动态过滤假期类型（keyword 模糊类型编码 / 类型名称，status 精确），
/// 按 id 降序（新建在前），恒排除软删。
///
/// `page_index` 为 0-based、`page_size` 已由 `PageQuery` clamp 到 1..=1000。
// 实现提示：`Condition::all()` 累加可选条件 → `Entity::find().filter(cond)`
// `.filter(Column::DeletedAt.is_null()).order_by_desc(Column::Id)` →
// `crate::utils::paginate(select, db, page_index, page_size)`。
pub async fn find_time_off_type_page(
    db: &impl ConnectionTrait,
    filter: &TimeOffTypeFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_time_off_type::Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = &filter.keyword {
        let like_keyword = format!("%{}%", keyword);
        cond = cond.add(
            Condition::any()
                .add(hr_time_off_type::Column::TypeCode.like(like_keyword.clone()))
                .add(hr_time_off_type::Column::TypeName.like(like_keyword)),
        )
    }

    if let Some(status) = filter.status {
        cond = cond.add(hr_time_off_type::Column::Status.eq(status));
    }

    let select = hr_time_off_type::Entity::find()
        .filter(cond)
        .filter(hr_time_off_type::Column::DeletedAt.is_null())
        .order_by_desc(hr_time_off_type::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查有效假期类型（排除软删）。
// 实现提示：`Entity::find().filter(Column::Id.eq(id)).filter(Column::DeletedAt.is_null()).one(db)`。
pub async fn find_time_off_type_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_time_off_type::Model>> {
    let model = hr_time_off_type::Entity::find()
        .filter(hr_time_off_type::Column::Id.eq(id))
        .filter(hr_time_off_type::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 按 ID 批量取假期类型（软删过滤：`DeletedAt.is_null()`；不分页；空入参直接返回空 `Vec`）。
///
/// 供 service 列表响应批量取名（`fill_time_off_type_names`）：单次查询，禁止逐行查库。
// 实现提示：`ids` 为空直接 `return Ok(Vec::new())` → `Entity::find().filter(Column::Id.is_in(ids))`
// `.filter(Column::DeletedAt.is_null()).all(db)`（顺序无要求，取名按 id 建 map 即可）。
pub async fn find_time_off_types_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> anyhow::Result<Vec<hr_time_off_type::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = hr_time_off_type::Entity::find()
        .filter(hr_time_off_type::Column::Id.is_in(ids.iter().copied()))
        .filter(hr_time_off_type::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(models)
}

/// 按 `type_code` 查假期类型——**含软删占位**（不过滤 `deleted_at`）。
///
/// `uk_hr_time_off_type_code` 是单列唯一键，软删行仍占位，查重必须能看到软删记录
/// （口径同 `sys_position.position_code`）；「编码已存在」的判定与文案在 service 层。
// 实现提示：不加 `DeletedAt.is_null()` 过滤，`.one(db)` 取唯一命中。
pub async fn find_time_off_type_by_code_include_deleted(
    db: &impl ConnectionTrait,
    code: &str,
) -> anyhow::Result<Option<hr_time_off_type::Model>> {
    let model = hr_time_off_type::Entity::find()
        .filter(hr_time_off_type::Column::TypeCode.eq(code))
        .one(db)
        .await?;
    Ok(model)
}

/// 事务内创建假期类型：审计盖章（创建人与更新人同源，均取 `actor_id`）。
// 实现提示：`ActiveModel { created_by: Set(actor_id), updated_by: Set(actor_id), ..model }`
// 后 `.insert(txn)`。
pub async fn create_time_off_type_in_tx(
    txn: &DatabaseTransaction,
    model: hr_time_off_type::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_time_off_type::Model> {
    let active_model = hr_time_off_type::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.insert(txn).await?;
    Ok(model)
}

/// 事务内更新假期类型（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
///
/// 入参 `model` 由 service 构造：只 Set 业务变更列，其余列留 `NotSet`。
// 实现提示：`ActiveModel { updated_by: Set(actor_id), ..model }.update(txn)`。
pub async fn update_time_off_type_in_tx(
    txn: &DatabaseTransaction,
    model: hr_time_off_type::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_time_off_type::Model> {
    let active_model = hr_time_off_type::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };
    let model = active_model.update(txn).await?;
    Ok(model)
}

/// 事务内软删假期类型：只更新 `deleted_at` + `updated_by`。
///
/// 返回是否有行被更新（`false` = 目标不存在或已软删）；
/// 「类型不存在」的判定与文案由 service 层负责。
// 实现提示：`update_many()` + `Column::Id.eq(id)` + `Column::DeletedAt.is_null()`
// （后者保证重复软删返回 false）+ `set(ActiveModel { deleted_at, updated_by, ..Default::default() })`。
pub async fn soft_delete_time_off_type_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let now = chrono::Local::now().naive_local();

    let result = hr_time_off_type::Entity::update_many()
        .filter(hr_time_off_type::Column::Id.eq(id))
        .filter(hr_time_off_type::Column::DeletedAt.is_null())
        .set(hr_time_off_type::ActiveModel {
            deleted_at: Set(Some(now)),
            updated_by: Set(actor_id),
            ..Default::default()
        })
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

/// FEFO 取批次：`status = 1`、`effective_at <= on_date`、
/// `(expire_at IS NULL OR expire_at >= on_date)`、`remaining_minutes > 0`，
/// 按 `expire_at ASC`（NULL 视为无穷大，排最后），`lock_exclusive()`。
///
/// 「读 → 判断 → 写」的额度原语专用：同一账户并发扣减靠行锁串行化。
// 实现提示：`.order_by_asc(Column::ExpireAt)` 无法把 NULL 排最后 → 用
// `sea_orm::sea_query::Expr::cust("expire_at IS NULL")` 作为第一排序键
// （`order_by_asc` 布尔表达式），再 `order_by_asc(Column::ExpireAt)`，
// 最后 `.lock_exclusive()`；`.all(txn)`。
// 同一失效日的批次（含多条 NULL）必须再补 `order_by_asc(Column::Id)` 兜底，
// 否则扣减顺序不确定（测试期望 NULL 组按 id 升序）。
pub async fn find_active_grants_for_update(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    on_date: Date,
) -> anyhow::Result<Vec<hr_time_off_grant::Model>> {
    let models = hr_time_off_grant::Entity::find()
        .filter(hr_time_off_grant::Column::EmployeeId.eq(employee_id))
        .filter(hr_time_off_grant::Column::TimeOffTypeId.eq(time_off_type_id))
        .filter(hr_time_off_grant::Column::Status.eq(1))
        .filter(hr_time_off_grant::Column::EffectiveAt.lte(on_date))
        .filter(hr_time_off_grant::Column::RemainingMinutes.gt(0))
        .filter(
            hr_time_off_grant::Column::ExpireAt
                .is_null()
                .or(hr_time_off_grant::Column::ExpireAt.gte(on_date)),
        )
        .order_by_asc(Expr::cust("expire_at IS NULL"))
        .order_by_asc(hr_time_off_grant::Column::ExpireAt)
        .order_by_asc(hr_time_off_grant::Column::Id)
        .lock_exclusive()
        .all(txn)
        .await?;
    Ok(models)
}

/// 扫描需作废的过期批次（**不加锁**）：`expire_at < today`、`remaining_minutes > 0`、
/// `status = 1`，按 `expire_at asc, id asc` 返回（expire job 用）。
///
/// 刻意不加 `FOR UPDATE`：过期是「全表按条件扫 + 逐行处理」，大范围行锁会与并发的发放 /
/// 预占互相等待甚至死锁（两个并发扫描会互等）。行级串行化交给调用方逐条
/// [`find_grant_by_id_for_update`] + 重判状态完成，重复执行因此天然幂等。
/// `expire_at` 为 NULL 的批次不满足 `lt(today)`，天然不入候选。
pub async fn find_expired_grants(
    txn: &DatabaseTransaction,
    today: Date,
) -> anyhow::Result<Vec<hr_time_off_grant::Model>> {
    let models = hr_time_off_grant::Entity::find()
        .filter(hr_time_off_grant::Column::ExpireAt.lt(today))
        .filter(hr_time_off_grant::Column::RemainingMinutes.gt(0))
        .filter(hr_time_off_grant::Column::Status.eq(1))
        .order_by_asc(hr_time_off_grant::Column::ExpireAt)
        .order_by_asc(hr_time_off_grant::Column::Id)
        .all(txn)
        .await?;
    Ok(models)
}

/// 按 id 加锁读批次（`SELECT ... FOR UPDATE`）。
///
/// 「读 → 判断 → 写」的额度原语专用；逐行处理（如过期）时先 `find_*` 拿候选、
/// 再逐条用本原语拿行锁并重判，避免宽范围加锁。
pub async fn find_grant_by_id_for_update(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<Option<hr_time_off_grant::Model>> {
    let model = hr_time_off_grant::Entity::find()
        .filter(hr_time_off_grant::Column::Id.eq(id))
        .lock_exclusive()
        .one(txn)
        .await?;
    Ok(model)
}

/// 事务内创建批次：审计盖章（创建人与更新人同源，均取 `actor_id`）。
// 实现提示：`ActiveModel { created_by: Set(actor_id), updated_by: Set(actor_id), ..model }.insert(txn)`。
pub async fn create_grant_in_tx(
    txn: &DatabaseTransaction,
    model: hr_time_off_grant::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_time_off_grant::Model> {
    let model = hr_time_off_grant::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    };
    let model = model.insert(txn).await?;
    Ok(model)
}

/// 按幂等键（员工 × 假别 × 依据 × 周期）查批次——**含已用尽 / 已失效行**。
///
/// 幂等判定必须能看到任何状态的历史批次，故不加 `status` 过滤。
// 实现提示：四列精确过滤 + `.one(txn)`；`idx_hr_time_off_grant_idempotent` 覆盖该查询。
pub async fn find_grant_by_idempotent_key(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    reason: &str,
    period: &str,
) -> anyhow::Result<Option<hr_time_off_grant::Model>> {
    let model = hr_time_off_grant::Entity::find()
        .filter(hr_time_off_grant::Column::EmployeeId.eq(employee_id))
        .filter(hr_time_off_grant::Column::TimeOffTypeId.eq(time_off_type_id))
        .filter(hr_time_off_grant::Column::Reason.eq(reason))
        .filter(hr_time_off_grant::Column::Period.eq(period))
        .one(txn)
        .await?;
    Ok(model)
}

/// 分页 + 动态过滤批次（employee_id / time_off_type_id / period / status 精确，
/// reason 模糊），按 id 降序（新批次在前）。
// 实现提示：`Condition::all()` 累加 → `.order_by_desc(Column::Id)` →
// `crate::utils::paginate(select, db, page_index, page_size)`；账本表无软删列，不加 `DeletedAt` 过滤。
pub async fn find_grant_page(
    db: &impl ConnectionTrait,
    filter: &TimeOffGrantFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_time_off_grant::Model>> {
    let mut cond = Condition::all();
    if let Some(employee_id) = filter.employee_id {
        cond = cond.add(hr_time_off_grant::Column::EmployeeId.eq(employee_id));
    }
    if let Some(time_off_type_id) = filter.time_off_type_id {
        cond = cond.add(hr_time_off_grant::Column::TimeOffTypeId.eq(time_off_type_id));
    }
    if let Some(period) = &filter.period {
        cond = cond.add(hr_time_off_grant::Column::Period.eq(period));
    }
    if let Some(status) = filter.status {
        cond = cond.add(hr_time_off_grant::Column::Status.eq(status));
    }
    if let Some(reason) = &filter.reason {
        cond = cond.add(hr_time_off_grant::Column::Reason.like(format!("%{}%", reason)));
    }
    let select = hr_time_off_grant::Entity::find()
        .filter(cond)
        .order_by_desc(hr_time_off_grant::Column::Id);
    let page = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page)
}

/// 按 id 查批次（详情用，不加锁）。
// 实现提示：`Entity::find().filter(Column::Id.eq(id)).one(db)`。
pub async fn find_grant_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_time_off_grant::Model>> {
    let model = hr_time_off_grant::Entity::find()
        .filter(hr_time_off_grant::Column::Id.eq(id))
        .one(db)
        .await?;
    Ok(model)
}

/// 扣批次剩余：`remaining_minutes >= minutes` 才更新，返回是否扣成功（并发护栏）。
///
/// 条件写进 `UPDATE ... WHERE` 而不先读后写，避免并发下扣成负数；
/// 扣到 0 之后的 `status` 翻转由 service 决定（本层只降剩余）。
// 实现提示：`update_many()` + `Column::Id.eq(grant_id)` + `Column::RemainingMinutes.gte(minutes)` +
// `col_expr(Column::RemainingMinutes, Expr::col(Column::RemainingMinutes) - minutes)` +
// `col_expr(Column::UpdatedBy, Expr::value(actor_id))`，返回 `rows_affected > 0`。
pub async fn consume_grant_in_tx(
    txn: &DatabaseTransaction,
    grant_id: u64,
    minutes: i32,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = hr_time_off_grant::Entity::update_many()
        .filter(hr_time_off_grant::Column::Id.eq(grant_id))
        .filter(hr_time_off_grant::Column::RemainingMinutes.gte(minutes))
        .col_expr(
            hr_time_off_grant::Column::RemainingMinutes,
            Expr::col(hr_time_off_grant::Column::RemainingMinutes).sub(minutes),
        )
        .col_expr(hr_time_off_grant::Column::UpdatedBy, Expr::value(actor_id))
        .exec(txn)
        .await?;

    Ok(result.rows_affected > 0)
}

/// 事务内刷新批次状态（2 已用尽 / 3 已失效 / 4 已撤销），返回是否有行被更新。
// 实现提示：`update_many()` + `Column::Id.eq(grant_id)` +
// `set(ActiveModel { status: Set(status), updated_by: Set(actor_id), ..Default::default() })`。
pub async fn set_grant_status_in_tx(
    txn: &DatabaseTransaction,
    grant_id: u64,
    status: i8,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = hr_time_off_grant::Entity::update_many()
        .filter(hr_time_off_grant::Column::Id.eq(grant_id))
        .col_expr(hr_time_off_grant::Column::Status, Expr::value(status))
        .col_expr(hr_time_off_grant::Column::UpdatedBy, Expr::value(actor_id))
        .exec(txn)
        .await?;

    Ok(result.rows_affected > 0)
}

/// 按账户唯一键（员工 × 假别 × 账期）**普通读**账户（不加锁，不加 `FOR UPDATE`）。
///
/// 用途：发放路径必须先判断账户是否存在——「存在才加锁读、不存在才插入」。
/// 如果对**不存在**的行先 `SELECT ... FOR UPDATE`（在 RR 下取间隙锁）再 `INSERT`，
/// 两个并发发放会各持同一间隙的间隙锁、再互相等待对方插入，形成死锁（MySQL 1213 →
/// `操作冲突，请稍后重试`）。普通读不取锁，从根上避开这条路径。
pub async fn find_balance_by_account(
    db: &impl ConnectionTrait,
    employee_id: u64,
    time_off_type_id: u64,
    period: &str,
) -> anyhow::Result<Option<hr_time_off_balance::Model>> {
    let model = hr_time_off_balance::Entity::find()
        .filter(hr_time_off_balance::Column::EmployeeId.eq(employee_id))
        .filter(hr_time_off_balance::Column::TimeOffTypeId.eq(time_off_type_id))
        .filter(hr_time_off_balance::Column::Period.eq(period))
        .one(db)
        .await?;
    Ok(model)
}

/// 按账户唯一键（员工 × 假别 × 账期）加锁读账户，`lock_exclusive()`。
///
/// 「读 → 判断 → 写」的额度原语专用：`uk_hr_time_off_balance_account` 保证唯一命中。
// 实现提示：三列精确过滤 + `.lock_exclusive()` + `.one(txn)`。
pub async fn find_balance_by_account_for_update(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    period: &str,
) -> anyhow::Result<Option<hr_time_off_balance::Model>> {
    let model = hr_time_off_balance::Entity::find()
        .filter(hr_time_off_balance::Column::EmployeeId.eq(employee_id))
        .filter(hr_time_off_balance::Column::TimeOffTypeId.eq(time_off_type_id))
        .filter(hr_time_off_balance::Column::Period.eq(period))
        .lock_exclusive()
        .one(txn)
        .await?;
    Ok(model)
}

/// 按 id 查账户（详情用，不加锁）。
// 实现提示：`Entity::find().filter(Column::Id.eq(id)).one(db)`。
pub async fn find_balance_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_time_off_balance::Model>> {
    let model = hr_time_off_balance::Entity::find()
        .filter(hr_time_off_balance::Column::Id.eq(id))
        .one(db)
        .await?;
    Ok(model)
}

/// 分页 + 动态过滤账户（employee_id / time_off_type_id / period 精确），按 id 降序。
// 实现提示：`Condition::all()` 累加 → `.order_by_desc(Column::Id)` →
// `crate::utils::paginate(select, db, page_index, page_size)`。
pub async fn find_balance_page(
    db: &impl ConnectionTrait,
    filter: &TimeOffBalanceFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_time_off_balance::Model>> {
    let mut cond = Condition::all();
    if let Some(employee_id) = filter.employee_id {
        cond = cond.add(hr_time_off_balance::Column::EmployeeId.eq(employee_id));
    }
    if let Some(time_off_type_id) = filter.time_off_type_id {
        cond = cond.add(hr_time_off_balance::Column::TimeOffTypeId.eq(time_off_type_id));
    }
    if let Some(period) = filter.period.as_deref() {
        cond = cond.add(hr_time_off_balance::Column::Period.eq(period));
    }
    let select = hr_time_off_balance::Entity::find()
        .filter(cond)
        .order_by_desc(hr_time_off_balance::Column::Id);
    let page = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page)
}

/// 事务内建账户（唯一键冲突即并发建户，由 service 重读）。
///
/// 无 `actor_id` 入参：审计列由 service 在 `ActiveModel` 里给定（账户由发放 / 系统动作创建）。
// 实现提示：`model.insert(txn)`。
pub async fn create_balance_in_tx(
    txn: &DatabaseTransaction,
    model: hr_time_off_balance::ActiveModel,
) -> anyhow::Result<hr_time_off_balance::Model> {
    let model = model.insert(txn).await?;
    Ok(model)
}

/// 事务内更新账户（窄写）：只刷新更新人，其余列以入参 `model` 为准。
///
/// 入参 `model` 由 service 构造：只 Set 变动列，其余列留 `NotSet`。
// 实现提示：`ActiveModel { updated_by: Set(actor_id), ..model }.update(txn)`。
pub async fn update_balance_in_tx(
    txn: &DatabaseTransaction,
    model: hr_time_off_balance::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_time_off_balance::Model> {
    let model = hr_time_off_balance::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    };
    let model = model.update(txn).await?;
    Ok(model)
}

/// 事务内追加一条流水（append-only：本层不提供更新 / 删除原语，冲正靠反向记录）。
///
/// 无 `actor_id` 入参：`operator_id` 是业务字段（0=系统），由 service 决定。
// 实现提示：`model.insert(txn)`。
pub async fn create_balance_log_in_tx(
    txn: &DatabaseTransaction,
    model: hr_time_off_balance_log::ActiveModel,
) -> anyhow::Result<hr_time_off_balance_log::Model> {
    Ok(model.insert(txn).await?)
}

/// 分页 + 动态过滤流水（employee_id / time_off_type_id / biz_type 精确），
/// 按 id 降序（最新在前）。
// 实现提示：`Condition::all()` 累加 → `.order_by_desc(Column::Id)` →
// `crate::utils::paginate(select, db, page_index, page_size)`；流水表无软删列。
pub async fn find_balance_log_page(
    db: &impl ConnectionTrait,
    filter: &TimeOffBalanceLogFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_time_off_balance_log::Model>> {
    let mut cond = Condition::all();

    if let Some(employee_id) = filter.employee_id {
        cond = cond.add(hr_time_off_balance_log::Column::EmployeeId.eq(employee_id));
    }

    if let Some(time_off_type_id) = filter.time_off_type_id {
        cond = cond.add(hr_time_off_balance_log::Column::TimeOffTypeId.eq(time_off_type_id));
    }

    if let Some(biz_type) = filter.biz_type {
        cond = cond.add(hr_time_off_balance_log::Column::BizType.eq(biz_type));
    }

    let select = hr_time_off_balance_log::Entity::find()
        .filter(cond)
        .order_by_desc(hr_time_off_balance_log::Column::Id);
    let page = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::hr_employee;
    use crate::modules::biz::hr::time_off::repo;
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同一进程内并行用例（repo / service 两个文件）必须互不相同，
    /// 否则撞 `uk_hr_time_off_type_code`。
    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一员工 ID：避开真实数据，且与 employee 域测试段位（900_1xx）不重叠。
    fn unique_employee_id() -> u64 {
        900_200_000
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed) % 1_000
    }

    fn date(y: i32, m: u32, d: u32) -> Date {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
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

    /// 测试用假期类型 ActiveModel（`type_code` 必须唯一，其余取最小值域）。
    fn time_off_type_model(type_code: String) -> hr_time_off_type::ActiveModel {
        hr_time_off_type::ActiveModel {
            type_code: Set(type_code),
            type_name: Set("测试假别".to_owned()),
            unit: Set(1),
            balance_mode: Set(1),
            min_unit_minutes: Set(240),
            status: Set(1),
            ..Default::default()
        }
    }

    /// 直插一份员工档案 + 建一个假期类型，返回 (employee_id, time_off_type_id)。
    ///
    /// 员工行：`hr_employee` 的非空列都有 DDL 默认值，只需 `user_id`（用
    /// `unique_employee_id() + 10_000` 避开真实用户，`uk_hr_employee_user_id` 单列唯一）。
    async fn seed_employee_and_type(txn: &DatabaseTransaction) -> (u64, u64) {
        seed_employee_and_type_with(txn, 0).await
    }

    /// 同上，但可指定 `allow_negative`（额度不足场景用）。
    async fn seed_employee_and_type_with(
        txn: &DatabaseTransaction,
        allow_negative: i8,
    ) -> (u64, u64) {
        let employee = hr_employee::ActiveModel {
            user_id: Set(unique_employee_id() + 10_000),
            employment_status: Set(1),
            education: Set(0),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap();

        let mut model = time_off_type_model(unique("lt"));
        model.allow_negative = Set(allow_negative);
        let time_off_type = repo::create_time_off_type_in_tx(txn, model, ACTOR_ID)
            .await
            .unwrap();

        (employee.id, time_off_type.id)
    }

    /// 测试用批次 ActiveModel：`remaining_minutes = minutes`、`status = 1`、
    /// `effective_at = 2026-01-01`、`source = 2`（手工调整）、`reason = manual`、`period = 2026`；
    /// 审计列交给 repo 盖章。
    fn grant_model(
        employee_id: u64,
        time_off_type_id: u64,
        minutes: i32,
        expire_at: Option<Date>,
    ) -> hr_time_off_grant::ActiveModel {
        hr_time_off_grant::ActiveModel {
            employee_id: Set(employee_id),
            time_off_type_id: Set(time_off_type_id),
            source: Set(2),
            reason: Set("manual".to_owned()),
            period: Set("2026".to_owned()),
            minutes: Set(minutes),
            remaining_minutes: Set(minutes),
            effective_at: Set(date(2026, 1, 1)),
            expire_at: Set(expire_at),
            status: Set(1),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn find_active_grants_for_update_returns_fefo_order_by_expire_at() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        // 三个批次：无失效日 / 2026-12-31 / 2026-06-30
        let far = repo::create_grant_in_tx(&txn, grant_model(emp, ty, 480, None), ACTOR_ID)
            .await
            .unwrap();
        let near = repo::create_grant_in_tx(
            &txn,
            grant_model(emp, ty, 480, Some(date(2026, 6, 30))),
            ACTOR_ID,
        )
        .await
        .unwrap();
        let forever = repo::create_grant_in_tx(&txn, grant_model(emp, ty, 480, None), ACTOR_ID)
            .await
            .unwrap();
        let grants = repo::find_active_grants_for_update(&txn, emp, ty, date(2026, 5, 1))
            .await
            .unwrap();
        let ids: Vec<u64> = grants.iter().map(|g| g.id).collect();
        assert_eq!(
            ids,
            vec![near.id, far.id, forever.id],
            "FEFO：先到期先扣，临期批次必须排在最前"
        );
    }

    #[tokio::test]
    async fn soft_delete_time_off_type_hides_it_from_find_by_id() {
        let txn = test_txn().await;
        let (_, ty) = seed_employee_and_type(&txn).await;

        assert!(
            repo::soft_delete_time_off_type_in_tx(&txn, ty, ACTOR_ID)
                .await
                .unwrap(),
            "首次软删必须返回 true"
        );
        assert!(
            !repo::soft_delete_time_off_type_in_tx(&txn, ty, ACTOR_ID)
                .await
                .unwrap(),
            "重复软删必须返回 false（已软删行不再命中）"
        );
        assert!(
            !repo::soft_delete_time_off_type_in_tx(&txn, ty + 900_000_000, ACTOR_ID)
                .await
                .unwrap(),
            "不存在的 id 必须返回 false"
        );
        assert!(
            repo::find_time_off_type_by_id(&txn, ty)
                .await
                .unwrap()
                .is_none(),
            "软删后 find_by_id 必须查不到"
        );
    }

    #[tokio::test]
    async fn create_time_off_type_stamps_both_audit_columns() {
        let txn = test_txn().await;
        let created = repo::create_time_off_type_in_tx(
            &txn,
            time_off_type_model(unique("ltstamp")),
            ACTOR_ID,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, ACTOR_ID, "创建人必须由 repo 盖章");
        assert_eq!(created.updated_by, ACTOR_ID, "创建时更新人必须与创建人同源");
    }

    #[tokio::test]
    async fn update_time_off_type_keeps_created_by_and_unset_columns() {
        let txn = test_txn().await;
        let created = repo::create_time_off_type_in_tx(
            &txn,
            time_off_type_model(unique("ltupdate")),
            ACTOR_ID,
        )
        .await
        .unwrap();
        let other_actor = ACTOR_ID + 7;

        let updated = repo::update_time_off_type_in_tx(
            &txn,
            hr_time_off_type::ActiveModel {
                id: Set(created.id),
                type_name: Set("改后假别名".to_owned()),
                ..Default::default()
            },
            other_actor,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, ACTOR_ID, "窄写更新不得覆盖创建人");
        assert_eq!(updated.updated_by, other_actor, "更新必须刷新更新人");
        assert_eq!(
            updated.type_code, created.type_code,
            "未 Set 的列必须保持原值"
        );
        assert_eq!(updated.type_name, "改后假别名", "Set 的列必须写入");
    }

    #[tokio::test]
    async fn find_time_off_type_page_filters_status_and_keyword() {
        let txn = test_txn().await;
        let kw = unique("ltpage");

        // 命中 type_code，启用
        let mut by_code = time_off_type_model(format!("{kw}_code"));
        by_code.type_name = Set("与此无关的名称".to_owned());
        let by_code = repo::create_time_off_type_in_tx(&txn, by_code, ACTOR_ID)
            .await
            .unwrap();

        // 只命中 type_name，启用
        let mut by_name = time_off_type_model(unique("ltpageother"));
        by_name.type_name = Set(format!("假别{kw}"));
        let by_name = repo::create_time_off_type_in_tx(&txn, by_name, ACTOR_ID)
            .await
            .unwrap();

        // 命中 keyword 但已停用：只有 status 过滤能排除它
        let mut disabled = time_off_type_model(format!("{kw}_off"));
        disabled.status = Set(0);
        let disabled = repo::create_time_off_type_in_tx(&txn, disabled, ACTOR_ID)
            .await
            .unwrap();

        // 命中 keyword 但已软删：keyword 与 status 都必须排除它
        let deleted = repo::create_time_off_type_in_tx(
            &txn,
            time_off_type_model(format!("{kw}_del")),
            ACTOR_ID,
        )
        .await
        .unwrap();
        repo::soft_delete_time_off_type_in_tx(&txn, deleted.id, ACTOR_ID)
            .await
            .unwrap();

        let all = repo::find_time_off_type_page(
            &txn,
            &TimeOffTypeFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let enabled = repo::find_time_off_type_page(
            &txn,
            &TimeOffTypeFilter {
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
    async fn find_balance_for_update_returns_none_for_missing_account_then_some_after_create() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;

        let missing = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap();
        assert!(missing.is_none(), "账户不存在必须返回 None");

        let created = repo::create_balance_in_tx(
            &txn,
            hr_time_off_balance::ActiveModel {
                employee_id: Set(emp),
                time_off_type_id: Set(ty),
                period: Set("2026".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let found = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .expect("建户后必须能按账户唯一键查到");
        assert_eq!(found.id, created.id);
        assert_eq!(found.employee_id, emp);
        assert_eq!(found.time_off_type_id, ty);
        assert_eq!(found.period, "2026");
    }

    #[tokio::test]
    async fn find_page_balance_log_orders_by_id_desc_and_filters_biz_type() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let balance = repo::create_balance_in_tx(
            &txn,
            hr_time_off_balance::ActiveModel {
                employee_id: Set(emp),
                time_off_type_id: Set(ty),
                period: Set("2026".to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // biz_type 1=授予（两条）、3=请假预占（一条）；常量在 mod.rs 由后续任务定义，此处用字面量
        let mut created_ids = Vec::new();
        for (biz_type, delta) in [(1_i8, 480_i32), (1_i8, 240_i32), (3_i8, -120_i32)] {
            let log = repo::create_balance_log_in_tx(
                &txn,
                hr_time_off_balance_log::ActiveModel {
                    balance_id: Set(balance.id),
                    employee_id: Set(emp),
                    time_off_type_id: Set(ty),
                    biz_type: Set(biz_type),
                    delta_minutes: Set(delta),
                    before_minutes: Set(0),
                    after_minutes: Set(delta),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            created_ids.push(log.id);
        }
        created_ids.sort_unstable_by(|a, b| b.cmp(a));

        let page = repo::find_balance_log_page(
            &txn,
            &TimeOffBalanceLogFilter {
                employee_id: Some(emp),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let ids: Vec<u64> = page.items.iter().map(|m| m.id).collect();
        assert_eq!(page.total, 3, "三条流水都必须命中");
        assert_eq!(ids, created_ids, "流水必须按 id 倒序（最新在前）");

        let granted = repo::find_balance_log_page(
            &txn,
            &TimeOffBalanceLogFilter {
                employee_id: Some(emp),
                biz_type: Some(1),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(granted.total, 2, "biz_type=1 应命中两条授予流水");
        assert!(
            granted.items.iter().all(|m| m.biz_type == 1),
            "biz_type 过滤不得漏出其他类型"
        );
    }
}
