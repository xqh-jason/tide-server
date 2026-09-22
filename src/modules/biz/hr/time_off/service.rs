//! 假期额度域业务：额度原语（发放 / 预占 / 实扣 / 释放 / 过期 / 可用查询）+ 资源 CRUD 编排。
//!
//! 分层约定（见 AGENTS.md「分层契约」）：
//! - 自持事务的入口收 `db: &DatabaseConnection`，内部 `begin` → 委托
//!   `pub(crate) *_in_tx(txn, …)` → 成功即 `commit`；`*_in_tx` 内**不得** begin / commit
//!   （事务边界由入口与测试外层事务负责）；
//! - 只读入口收 `&impl ConnectionTrait`，不起事务；
//! - 额度原语是 P2 请假 / P4 加班的**共用写入口**：账本两条不变式由本层维护——
//!   A：`Σ log.delta_minutes == granted + adjust − used − locked − expired`；
//!   B：`Σ 未失效批次的 remaining_minutes == granted − used`；
//! - 扣减恒定 FEFO（先到期先扣）：顺序由 `repo::find_active_grants_for_update` 保证，
//!   批次级并发护栏在 `repo::consume_grant_in_tx` 的 `remaining_minutes >= minutes` 条件里；
//! - 账户账期（`period`）口径见下文「账户账期」一节——一个账户 = 一个自然年的额度桶。
//!
//! # 账本口径（`hr_time_off_balance_log.delta_minutes`）
//!
//! `delta_minutes` 恒等于该动作对**可用余额**的影响（= `after_minutes − before_minutes`），
//! 与 `hr_time_off_balance_log` 的 `before_minutes` / `after_minutes` 列注释同构：
//!
//! | biz_type | delta | 账户字段变化 | 可用余额变化 |
//! |---|---|---|---|
//! | 1 授予 | `+minutes` | `granted += m` | +m |
//! | 2 手工调整 | `±minutes` | `adjust ± m` | ±m |
//! | 3 请假预占 | `−minutes` | `locked += m` | −m |
//! | 4 审批实扣 | `0` | `locked −= m`、`used += m` | 0（仅批次归属转移，靠 `grant_id` 记录扣了哪一批） |
//! | 5 驳回释放 | `+minutes` | `locked −= m` | +m |
//! | 6 过期作废 | `−minutes` | `expired += m` | −m |
//!
//! 因此「流水净额 == 账户净额」的不变式 A 对任意操作序列都成立；实扣写 0 而非 `−m`，
//! 是为了不与预占的 `−m` 重复计数（预占已经扣过可用）。
//!
//! # 账户账期（`hr_time_off_balance.period`）
//!
//! 账户账期 = **交易发生日 / 发放生效日的自然年**：
//! - `grant_time_off_in_tx` 取 `effective_at` 的自然年——`hr_time_off_grant.period` 只是**归属标记**
//!   （幂等键与展示用），**不决定账户**；
//! - `lock` / `consume` / `release` 取各自 `on_date` 的自然年；`expire` 取批次
//!   `effective_at` 的自然年——必须与它发放时入账的账户同桶（作废若落到 `today`
//!   的年度，批次的剩余被清零而原授予桶仍留下可用量，跨年作废时 A / B 不变式必破）。
//!
//! 反例（为什么结转批次不能拿 `period` 定账户）：上一年度结转的额度批次 `period = "2025"`、
//! `effective_at = 2026-01-01`，它在本年度可用，必须计入 `2026` 账期的 `granted`——
//! 若按 `period` 落进 `2025` 账户，本年度的可用额度会凭空少一块，且 FEFO 扣减（不按账期过滤）
//! 扣掉它的剩余时，A / B 两条不变式必然破。测试里：`lock_then_consume_…` 的 `granted == 960`
//! 钉死了「结转批次入本年度账户」；`expire_…` 的 `expired == 480` 因 `today`（2026-04-01）与批次
//! `effective_at`（2026-01-01）同年度，只钉死「作废不按 `period` 字段入账」；真正区分
//! 「与授予同桶」和「按 `today` 入账」的是用例 `expire_grants_books_to_the_same_account_as_the_grant_across_years`。
//!
//! # 记录型假别（`balance_mode = BALANCE_MODE_RECORD_ONLY`）
//!
//! 记录型假别（只记录不扣额度，如种子里的 `personal` 事假 / `sick` 病假）**没有额度概念**：
//! - 调用方（P2 请假 service）在提交时读假别 `balance_mode`，为 `BALANCE_MODE_RECORD_ONLY`
//!   时整条额度链路跳过——不 lock / 不 consume / 不 release / 不写流水；
//! - 原语本层保留**防御性 no-op**：[`lock_time_off_in_tx`] / [`consume_locked_in_tx`] /
//!   [`release_locked_in_tx`] 读到 `balance_mode = 0` 直接 `Ok(())`（不建账户、不写流水），
//!   [`available_minutes`] 返回 `Ok(0)`。
//!
//! **P2 请假计划需同步该口径**（否则事假 / 病假会被本层的「账户不存在」护栏挡住）。

use std::collections::HashMap;

use sea_orm::ActiveValue::Set;
use sea_orm::DatabaseTransaction;
use sea_orm::entity::prelude::*;

use crate::entity::{
    hr_time_off_balance, hr_time_off_balance_log, hr_time_off_grant, hr_time_off_type,
};
use crate::modules::biz::hr::time_off::dto::{
    BatchCreateGrantReq, BatchCreateGrantResp, CreateTimeOffTypeReq, TimeOffBalanceFilter,
    TimeOffBalanceListReq, TimeOffBalanceLogFilter, TimeOffBalanceLogListReq, TimeOffGrantFilter,
    TimeOffGrantListReq, TimeOffTypeFilter, TimeOffTypeListReq, UpdateTimeOffTypeReq,
};
use crate::modules::biz::hr::time_off::{
    BALANCE_MODE_RECORD_ONLY, GRANT_SOURCE_ISSUE, GRANT_STATUS_ACTIVE, GRANT_STATUS_CANCELED,
    GRANT_STATUS_EXHAUSTED, GRANT_STATUS_EXPIRED, LOG_BIZ_ADJUST, LOG_BIZ_EXPIRE, LOG_BIZ_GRANT,
    LOG_BIZ_TIME_OFF_CONSUME, LOG_BIZ_TIME_OFF_LOCK, LOG_BIZ_TIME_OFF_RELEASE, LOG_SOURCE_JOB,
    LOG_SOURCE_MANUAL, LOG_SOURCE_NONE, repo as time_off_repo,
};
use crate::utils::PageData;
use crate::utils::error::AppError;
use chrono::Datelike;

// —— 额度原语（pub(crate)：P2 请假 / P4 加班复用；调用方必须自持事务）——

/// 发放额度：幂等键（员工 × 假别 × 依据 × 周期）命中即复用，否则建批次 + 记账户 + 写流水。
///
/// 返回批次 ID。调用方（[`batch_create_grants_in_tx`] / P2 请假 / P4 加班）负责事务边界。
// 实现提示：① 幂等探测 `repo::find_grant_by_idempotent_key(txn, employee_id, time_off_type_id, reason, period)`，
// 命中直接返回其 `id`（不重复累加任何账户字段）；② 账户加锁读
// `repo::find_balance_by_account_for_update`，账期取 `effective_at` 的自然年
// （`format!("{}", effective_at.year())`，需 `use chrono::Datelike`），为 `None` 则
// `repo::create_balance_in_tx` 建零账户（审计列在 ActiveModel 里给定 actor_id）；
// ③ `repo::create_grant_in_tx` 建批次：`remaining_minutes = minutes`、
// `status = GRANT_STATUS_ACTIVE`、`source` / `reason` / `period` / `effective_at` / `expire_at` 入参直落；
// ④ 账户 `granted_minutes += minutes` → `repo::update_balance_in_tx`（窄写，只 Set 变动列）；
// ⑤ `repo::create_balance_log_in_tx` 写流水：`biz_type = LOG_BIZ_GRANT`、`delta = +minutes`
// （授予增加可用，见文件头账本口径）、`before/after` 取变动前后可用值、`grant_id` = 新批次、
// `source_kind` = 4（手工）/ 1（系统任务，按调用场景）、`operator_id = actor_id`。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
// 成对原语的入参集合固定；拆参数结构体会让跨域调用方多一层构造，收益不足
#[allow(clippy::too_many_arguments)]
pub(crate) async fn grant_time_off_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    employee_id: u64,
    time_off_type_id: u64,
    minutes: i32,
    source: i8,
    reason: &str,
    period: &str,
    effective_at: Date,
    expire_at: Option<Date>,
) -> Result<u64, AppError> {
    // ① 幂等：同「员工 × 假别 × 依据 × 周期」已发过就复用，不重复累加任何账户字段
    let existing = time_off_repo::find_grant_by_idempotent_key(
        txn,
        employee_id,
        time_off_type_id,
        reason,
        period,
    )
    .await?;
    if let Some(existing) = existing {
        return Ok(existing.id);
    }

    // ② 账户：不存在则开户，存在则加锁读（账期 = 发放生效日的自然年；
    //    `period` 入参只是归属标记，不决定账户）
    let account_period = effective_at.year().to_string();
    let balance = lock_or_create_balance_in_tx(
        txn,
        actor_id,
        employee_id,
        time_off_type_id,
        &account_period,
    )
    .await?;

    // ③ 批次：`minutes` 是授予总量（NOT NULL），`remaining_minutes` 是剩余，两者都写
    let grant = time_off_repo::create_grant_in_tx(
        txn,
        hr_time_off_grant::ActiveModel {
            employee_id: Set(employee_id),
            time_off_type_id: Set(time_off_type_id),
            source: Set(source),
            reason: Set(reason.to_string()),
            period: Set(period.to_string()),
            minutes: Set(minutes),
            remaining_minutes: Set(minutes),
            effective_at: Set(effective_at),
            expire_at: Set(expire_at),
            status: Set(GRANT_STATUS_ACTIVE),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    // ④ 账户 `granted += minutes`
    let before = available_of(&balance);
    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            granted_minutes: Set(balance.granted_minutes + minutes),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    // ⑤ 授予流水：`delta = +minutes`（授予增加可用）
    append_balance_log_in_tx(
        txn,
        BalanceLog {
            balance_id: balance.id,
            employee_id,
            time_off_type_id,
            grant_id: grant.id,
            biz_type: LOG_BIZ_GRANT,
            before_minutes: before,
            after_minutes: before + i64::from(minutes),
            delta_minutes: minutes,
            source_kind: LOG_SOURCE_MANUAL,
            source_id: 0,
            operator_id: actor_id,
            remark: "",
        },
    )
    .await?;

    Ok(grant.id)
}

/// 在职状态「离职」（字典 `employmentStatus` 的 3）：批量发放 `all` 范围要排除离职员工。
const EMPLOYMENT_STATUS_RESIGNED: i8 = 3;

/// 解析 `yyyy-MM-dd` 日期（格式已在 `validate.rs` 拦过，这里是进入 service 后的兜底）。
fn parse_date(raw: &str) -> Result<Date, AppError> {
    chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::Biz(format!("日期格式不正确：{raw}")))
}

/// 解析批量发放的目标员工（三种范围互斥，互斥性由 `validate.rs` 保证）。
///
/// - `all`：分页遍历在职员工（排除离职）；
/// - `dept_id`：该部门挂载的用户 → 各自的员工档案；
/// - 否则：直接用 `employee_ids`。
///
/// 三种口径都去重且保持稳定顺序（`skipped_employee_ids` 的回执顺序依赖它）。
async fn resolve_grant_targets(
    txn: &DatabaseTransaction,
    req: &BatchCreateGrantReq,
) -> Result<Vec<u64>, AppError> {
    use crate::modules::biz::hr::employee::dto::EmployeeFilter;
    use crate::modules::biz::hr::employee::repo as employee_repo;

    if req.all {
        let mut ids = Vec::new();
        let mut page_index = 0u64;
        loop {
            // page_size 上限 1000（`PageQuery::page_size` 同口径）；page_index 0-based
            let page = employee_repo::find_employee_page(
                txn,
                &EmployeeFilter::default(),
                page_index,
                1000,
            )
            .await?;
            let fetched = page.items.len() as u64;
            ids.extend(
                page.items
                    .into_iter()
                    .filter(|employee| employee.employment_status != EMPLOYMENT_STATUS_RESIGNED)
                    .map(|employee| employee.id),
            );
            if fetched == 0 || (page_index + 1) * 1000 >= page.total {
                break;
            }
            page_index += 1;
        }
        return Ok(crate::utils::user_ref::dedup_ids(ids));
    }

    if let Some(dept_id) = req.dept_id {
        let user_ids =
            crate::modules::system::user::repo::find_user_ids_by_dept_id(txn, dept_id).await?;
        let employees = employee_repo::find_by_user_ids(txn, &user_ids).await?;
        let employee_by_user: HashMap<u64, u64> =
            employees.into_iter().map(|e| (e.user_id, e.id)).collect();
        // 按 user_ids 顺序回填，保序且自然去重
        return Ok(crate::utils::user_ref::dedup_ids(
            user_ids
                .into_iter()
                .filter_map(|user_id| employee_by_user.get(&user_id).copied())
                .collect(),
        ));
    }

    Ok(crate::utils::user_ref::dedup_ids(req.employee_ids.clone()))
}

// —— 私有 helper：把「取账户」「写流水」这两件重复且易错的事各收口一次 ——

/// 取账户：不存在则开户，存在则加锁读。
///
/// **顺序不能反**：若对**不存在**的行先 `SELECT ... FOR UPDATE`，RR 隔离级别下会取到
/// 间隙锁，两个并发发放各持同一间隙的锁、再互相等待对方 `INSERT` → 死锁（MySQL 1213，
/// 表现为 `操作冲突，请稍后重试`）。因此固定为「普通读判存在 → 存在才加锁 / 不存在才插入」；
/// 插入撞唯一键（并发建户）时加锁重读，拿回对方刚建的账户。
async fn lock_or_create_balance_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    employee_id: u64,
    time_off_type_id: u64,
    period: &str,
) -> Result<hr_time_off_balance::Model, AppError> {
    let opened = time_off_repo::find_balance_by_account(txn, employee_id, time_off_type_id, period)
        .await?
        .is_some();
    if opened {
        return lock_balance_in_tx(txn, employee_id, time_off_type_id, period).await;
    }

    // 未开户：直接插入（五个分钟列走 DDL 默认 0，审计列由本层给定 actor）
    let new_account = hr_time_off_balance::ActiveModel {
        employee_id: Set(employee_id),
        time_off_type_id: Set(time_off_type_id),
        period: Set(period.to_string()),
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..Default::default()
    };
    match time_off_repo::create_balance_in_tx(txn, new_account).await {
        Ok(balance) => Ok(balance),
        // 并发建户：重读成功即以对方建的账户为准；重读也失败才抛插入的原始错误（根因）
        Err(insert_err) => lock_balance_in_tx(txn, employee_id, time_off_type_id, period)
            .await
            .map_err(|_| AppError::from(insert_err)),
    }
}

/// 加锁读账户（`SELECT ... FOR UPDATE`）；不存在即报错。
///
/// 只用于「账户必然已存在」的路径（加锁读不存在的行会取间隙锁，见上）。
async fn lock_balance_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    period: &str,
) -> Result<hr_time_off_balance::Model, AppError> {
    time_off_repo::find_balance_by_account_for_update(txn, employee_id, time_off_type_id, period)
        .await?
        .ok_or_else(|| AppError::Biz("额度账户不存在，请先发放额度".into()))
}

/// 一条额度流水的画像：只描述「这笔变动是什么」，账户 / 人员由调用点填。
struct BalanceLog<'a> {
    balance_id: u64,
    employee_id: u64,
    time_off_type_id: u64,
    /// 涉及的授予批次；账户级动作（预占 / 释放）填 0
    grant_id: u64,
    biz_type: i8,
    /// 可用余额的变动量（= `after_minutes − before_minutes`，口径见文件头「账本口径」）
    delta_minutes: i32,
    before_minutes: i64,
    after_minutes: i64,
    source_kind: i8,
    source_id: u64,
    operator_id: u64,
    remark: &'a str,
}

/// 追加一条流水（append-only）。
///
/// 收口两件事：① `i64` 可用值 → `i32` 列的转换只在这里做一次；
/// ② 断言 `delta == after − before`，把「流水口径」钉在编译期的 debug 断言上，
/// 任何算错可用值 / 记错 delta 的调用都会在测试里立刻暴露。
async fn append_balance_log_in_tx(
    txn: &DatabaseTransaction,
    log: BalanceLog<'_>,
) -> Result<(), AppError> {
    debug_assert_eq!(
        i64::from(log.delta_minutes),
        log.after_minutes - log.before_minutes,
        "流水 delta 必须等于可用余额变动量（after − before）"
    );

    time_off_repo::create_balance_log_in_tx(
        txn,
        hr_time_off_balance_log::ActiveModel {
            balance_id: Set(log.balance_id),
            employee_id: Set(log.employee_id),
            time_off_type_id: Set(log.time_off_type_id),
            grant_id: Set(log.grant_id),
            biz_type: Set(log.biz_type),
            delta_minutes: Set(log.delta_minutes),
            before_minutes: Set(log.before_minutes as i32),
            after_minutes: Set(log.after_minutes as i32),
            source_kind: Set(log.source_kind),
            source_id: Set(log.source_id),
            operator_id: Set(log.operator_id),
            remark: Set(log.remark.to_string()),
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

/// 账户可用额度（分钟）：`granted + adjust − used − locked − expired`。
///
/// 账户各列是 `i32`，相减可能越界，故聚合口径一律升位到 `i64`；
/// 本函数是「可用」的唯一算式，[`lock_time_off_in_tx`] / [`available_minutes`] 等一律复用它，
/// 避免多处各写一遍后漂移。
fn available_of(balance: &hr_time_off_balance::Model) -> i64 {
    i64::from(balance.granted_minutes) + i64::from(balance.adjust_minutes)
        - i64::from(balance.used_minutes)
        - i64::from(balance.locked_minutes)
        - i64::from(balance.expired_minutes)
}

/// 预占额度（审批中）：账户 `locked_minutes += minutes`，可用不足且假别 `allow_negative = 0` 时拒绝。
///
/// 预占只动 `locked`，**不**动 `used`——实扣在审批通过时由 [`consume_locked_in_tx`] 完成。
// 实现提示：⓪ 先读假别 `repo::find_time_off_type_by_id(txn, time_off_type_id)` 取 `balance_mode` 与
// `allow_negative`：`BALANCE_MODE_RECORD_ONLY` → 直接 `Ok(())`（记录型假别无额度概念，
// 防御性 no-op，见文件头「记录型假别」）；① 账户加锁读
// `repo::find_balance_by_account_for_update(txn, employee_id, time_off_type_id, period)`，账期取
// `on_date` 的自然年；`None` → `AppError::Biz("额度账户不存在，请先发放额度")`；
// ② 账户加锁读后即可用（`allow_negative` 已在 ⓪ 取到）；
// ③ `available = granted + adjust − used − locked − expired`；`available < minutes` 且
// `allow_negative == 0` → `AppError::Biz(format!("额度不足：可用 {available} 分钟，本次需要 {minutes} 分钟"))`
// （`allow_negative == 1` 放行，允许负余额）；④ 账户 `locked_minutes += minutes` →
// `repo::update_balance_in_tx`；⑤ 写流水：`biz_type = LOG_BIZ_TIME_OFF_LOCK`、`delta = −minutes`
// （预占把可用锁住，见文件头账本口径）、`grant_id = 0`（账户级动作）、`source_kind = 0` /
// `source_id = 0`（本签名不带来源单据，P2 接入请假单后由调用方补）。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn lock_time_off_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    minutes: i32,
    on_date: Date,
) -> Result<(), AppError> {
    let type_model = time_off_repo::find_time_off_type_by_id(txn, time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{time_off_type_id}")))?;

    // 记录型假期无额度概念
    if type_model.balance_mode == BALANCE_MODE_RECORD_ONLY {
        return Ok(());
    }

    let period = on_date.year().to_string();

    let balance = lock_balance_in_tx(txn, employee_id, time_off_type_id, &period).await?;

    let before = available_of(&balance);
    if before < i64::from(minutes) && type_model.allow_negative == 0 {
        return Err(AppError::Biz(format!(
            "额度不足：可用 {before} 分钟，本次需要 {minutes} 分钟"
        )));
    }

    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            locked_minutes: Set(balance.locked_minutes + minutes),
            ..Default::default()
        },
        0, // 本签名不带 actor_id：账户审计列记 0（系统）；P2 若要记审批人，扩签名
    )
    .await?;

    append_balance_log_in_tx(
        txn,
        BalanceLog {
            balance_id: balance.id,
            employee_id,
            time_off_type_id,
            grant_id: 0, // 账户级动作：预占不落到具体批次
            biz_type: LOG_BIZ_TIME_OFF_LOCK,
            before_minutes: before,
            after_minutes: before - i64::from(minutes),
            delta_minutes: -minutes,
            source_kind: LOG_SOURCE_NONE,
            source_id: 0, // 本签名不带来源单据，P2 接入请假单后由调用方补
            operator_id: 0,
            remark: "",
        },
    )
    .await?;

    Ok(())
}

/// 实扣（审批通过）：FEFO 扣批次剩余，账户 `locked -= minutes`、`used += minutes`，可用不变。
///
/// 批次归属：每个被扣的批次写一条流水（`grant_id` 指向它），供「这笔假扣的是哪个批次」追溯。
// 实现提示：⓪ 读假别 `repo::find_time_off_type_by_id(txn, time_off_type_id)`：`balance_mode` 为
// `BALANCE_MODE_RECORD_ONLY` → 直接 `Ok(())`（记录型假别无额度概念，防御性 no-op）；
// ① `repo::find_active_grants_for_update(txn, employee_id, time_off_type_id, on_date)` FEFO 加锁读；
// ② 账户加锁读（账期取 `on_date` 的自然年）——`locked_minutes -= minutes`、`used_minutes += minutes`，
// 变动前后**可用值相同**；③ 循环扣批次：`take = min(剩余待扣, batch.remaining_minutes)` →
// `repo::consume_grant_in_tx(txn, batch.id, take, actor_id)`（返回 `false` 说明并发下批次已被扣空，
// 重新取批次或直接报错）；扣不满 `minutes` → `AppError::Biz("额度批次不足，请检查账本一致性")`；
// ④ 每个被扣批次：`remaining_minutes == 0` 时
// `repo::set_grant_status_in_tx(txn, batch.id, GRANT_STATUS_EXHAUSTED, actor_id)`，
// 并写一条流水：`biz_type = LOG_BIZ_TIME_OFF_CONSUME`、`delta = 0`（locked→used，可用不变，
// 见文件头账本口径）、`grant_id = batch.id`、`source_kind` / `source_id` 原样入来源列；
// ⑤ `grant_id` 之外的 `operator_id` 传 0（系统动作），P2 接入请假单后可改为审批人。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn consume_locked_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    minutes: i32,
    source_kind: i8,
    source_id: u64,
    on_date: Date,
) -> Result<(), AppError> {
    let type_model = time_off_repo::find_time_off_type_by_id(txn, time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{time_off_type_id}")))?;
    // 记录型假别无额度概念
    if type_model.balance_mode == BALANCE_MODE_RECORD_ONLY {
        return Ok(());
    }

    let period = on_date.year().to_string();
    let balance = lock_balance_in_tx(txn, employee_id, time_off_type_id, &period).await?;
    // locked → used：可用值不变（见文件头账本口径）
    let before = available_of(&balance);

    // 先扣批次：扣不满就整笔失败，账户改动随事务回滚，避免账本出现「扣了一半」
    let batches =
        time_off_repo::find_active_grants_for_update(txn, employee_id, time_off_type_id, on_date)
            .await?;
    let mut need = minutes;
    for batch in batches {
        if need == 0 {
            break;
        }
        let take = need.min(batch.remaining_minutes);
        // 批次级并发护栏在 repo 的 `remaining >= minutes` 条件里；false = 已被并发扣空
        // 本签名不带操作人：批次侧审计记 0（系统动作）
        if !time_off_repo::consume_grant_in_tx(txn, batch.id, take, 0).await? {
            return Err(AppError::Biz("额度批次不足，请检查账本一致性".into()));
        }
        if take == batch.remaining_minutes {
            time_off_repo::set_grant_status_in_tx(txn, batch.id, GRANT_STATUS_EXHAUSTED, 0).await?;
        }
        // 每个被扣批次一条流水：delta = 0（可用不变），grant_id 指向它以便追溯
        append_balance_log_in_tx(
            txn,
            BalanceLog {
                balance_id: balance.id,
                employee_id,
                time_off_type_id,
                grant_id: batch.id,
                biz_type: LOG_BIZ_TIME_OFF_CONSUME,
                before_minutes: before,
                after_minutes: before,
                delta_minutes: 0,
                source_kind,
                source_id,
                operator_id: 0,
                remark: "",
            },
        )
        .await?;
        need -= take;
    }
    if need > 0 {
        return Err(AppError::Biz("额度批次不足，请检查账本一致性".into()));
    }

    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            locked_minutes: Set(balance.locked_minutes - minutes),
            used_minutes: Set(balance.used_minutes + minutes),
            ..Default::default()
        },
        0, // 本签名不带操作人：系统动作
    )
    .await?;
    Ok(())
}

/// 释放预占（审批驳回 / 撤销）：账户 `locked -= minutes`，可用恢复，流水正向记恢复量。
// 实现提示：⓪ 读假别 `repo::find_time_off_type_by_id(txn, time_off_type_id)`：`balance_mode` 为
// `BALANCE_MODE_RECORD_ONLY` → 直接 `Ok(())`（记录型假别无额度概念，防御性 no-op）；
// ① 账户加锁读 `repo::find_balance_by_account_for_update(txn, employee_id, time_off_type_id, period)`，
// 账期取 `on_date` 的自然年（`format!("{}", on_date.year())`）；`None` →
// `AppError::Biz("额度账户不存在，请先发放额度")`；② 账户 `locked_minutes -= minutes` →
// `repo::update_balance_in_tx`；③ 写流水：`biz_type = LOG_BIZ_TIME_OFF_RELEASE`、`delta = +minutes`
// （可用恢复，见文件头账本口径）、`grant_id = 0`（账户级）、`source_kind` / `source_id` 原样入来源列。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn release_locked_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    minutes: i32,
    source_kind: i8,
    source_id: u64,
    on_date: Date,
) -> Result<(), AppError> {
    let type_model = time_off_repo::find_time_off_type_by_id(txn, time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{time_off_type_id}")))?;
    // 记录型假别无额度概念
    if type_model.balance_mode == BALANCE_MODE_RECORD_ONLY {
        return Ok(());
    }

    let period = on_date.year().to_string();
    let balance = lock_balance_in_tx(txn, employee_id, time_off_type_id, &period).await?;
    if i64::from(balance.locked_minutes) < i64::from(minutes) {
        return Err(AppError::Biz("释放量超过预占量，请检查账本一致性".into()));
    }

    let before = available_of(&balance);
    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            locked_minutes: Set(balance.locked_minutes - minutes),
            ..Default::default()
        },
        0, // 本签名不带操作人：系统动作
    )
    .await?;

    append_balance_log_in_tx(
        txn,
        BalanceLog {
            balance_id: balance.id,
            employee_id,
            time_off_type_id,
            grant_id: 0, // 账户级动作：预占不落到具体批次
            biz_type: LOG_BIZ_TIME_OFF_RELEASE,
            before_minutes: before,
            after_minutes: before + i64::from(minutes),
            delta_minutes: minutes,
            source_kind,
            source_id,
            operator_id: 0,
            remark: "",
        },
    )
    .await?;
    Ok(())
}

/// 批量作废过期批次（定时任务调用）：`expire_at < today` 且仍有剩余的批次归零 + 置失效 + 记流水。
///
/// 返回本次作废的批次数。幂等靠 `remaining_minutes > 0` 的扫描条件——重复跑不会二次记账。
// 实现提示：① `repo::find_expired_grants_for_update(txn, today)` 加锁扫描候选
// （`expire_at < today` + `remaining_minutes > 0` + `status = 1`，按 `expire_at asc, id asc`）；
// ② 逐条：账户加锁读（**账期取批次 `effective_at` 的自然年**——必须与发放时入账的账户同桶：
// 取 `today` 在跨年作废时会把「批次清零」与「授予桶」拆进两个年度，取批次 `period` 字段则与
// 授予口径不一致，两者都会破文件头「账户账期」的不变式）`expired_minutes += remaining` →
// 批次 `remaining_minutes` 归零 + `repo::set_grant_status_in_tx(txn, id, GRANT_STATUS_EXPIRED, 0)`
// （系统任务，actor_id 传 0）；③ 每批次写一条流水：`biz_type = LOG_BIZ_EXPIRE`、
// `delta = −remaining`（作废减少可用，见文件头账本口径）、`grant_id` = 批次 id、
// `source_kind = 1`（系统任务）、`operator_id = 0`；④ 账户变动经 `repo::update_balance_in_tx` 落库。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
pub(crate) async fn expire_grants_in_tx(
    txn: &DatabaseTransaction,
    today: Date,
) -> Result<u64, AppError> {
    let candidates = time_off_repo::find_expired_grants(txn, today).await?;
    let mut expired_count = 0u64;

    for candidate in candidates {
        // 逐行加锁 + 重判：并发执行（或重复执行）下已被处理的批次直接跳过
        let Some(batch) = time_off_repo::find_grant_by_id_for_update(txn, candidate.id).await?
        else {
            continue;
        };
        if batch.status != GRANT_STATUS_ACTIVE || batch.remaining_minutes <= 0 {
            continue;
        }

        // 账期取批次 `effective_at` 的自然年：必须与它发放时入账的账户同桶（跨年作废不拆桶）
        let period = batch.effective_at.year().to_string();
        let balance = lock_balance_in_tx(txn, batch.employee_id, batch.time_off_type_id, &period)
            .await
            .map_err(|_| AppError::Biz(format!("额度账户不存在，账本异常：批次 {}", batch.id)))?;

        let before = available_of(&balance);
        let remaining = batch.remaining_minutes;

        time_off_repo::update_balance_in_tx(
            txn,
            hr_time_off_balance::ActiveModel {
                id: Set(balance.id),
                expired_minutes: Set(balance.expired_minutes + remaining),
                ..Default::default()
            },
            0, // 系统任务
        )
        .await?;
        time_off_repo::consume_grant_in_tx(txn, batch.id, remaining, 0).await?;
        time_off_repo::set_grant_status_in_tx(txn, batch.id, GRANT_STATUS_EXPIRED, 0).await?;

        append_balance_log_in_tx(
            txn,
            BalanceLog {
                balance_id: balance.id,
                employee_id: batch.employee_id,
                time_off_type_id: batch.time_off_type_id,
                grant_id: batch.id,
                biz_type: LOG_BIZ_EXPIRE,
                before_minutes: before,
                after_minutes: before - i64::from(remaining),
                delta_minutes: -remaining,
                source_kind: LOG_SOURCE_JOB,
                source_id: 0,
                operator_id: 0,
                remark: "过期作废",
            },
        )
        .await?;
        expired_count += 1;
    }

    Ok(expired_count)
}

/// 可用额度（分钟）：账户口径 `granted + adjust − used − locked − expired`。
///
/// `period` 为账期（自然年字符串，调用方按「单据发生日」解析）。返回 `i64`：账户各列是 `i32`，
/// 相减可能越界，聚合口径一律升位到 `i64`。
// 实现提示：① 本签名收 `&DatabaseTransaction`，与加锁读原语同型，直接复用
// `repo::find_balance_by_account_for_update(txn, employee_id, time_off_type_id, period)`（无需新 SQL）；
// ② 账户 `None` 视为 0（未发放即无可用量）；③ 读假别 `repo::find_time_off_type_by_id`：
// `balance_mode = BALANCE_MODE_RECORD_ONLY` → 返回 `Ok(0)`，并**加注释**
// 「记录型假别无额度概念，调用方不应据此判定可否请假」（见文件头「记录型假别」）；
// ④ `on_date` 当前不参与计算（签名保留，供后续「按有效期折算可用」口径），
// 实现时 `let _ = on_date;` 消音。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn available_minutes(
    txn: &DatabaseTransaction,
    employee_id: u64,
    time_off_type_id: u64,
    period: &str,
    on_date: Date,
) -> Result<i64, AppError> {
    let type_model = time_off_repo::find_time_off_type_by_id(txn, time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{time_off_type_id}")))?;
    // 记录型假别无额度概念，调用方不应据此判定可否请假
    if type_model.balance_mode == BALANCE_MODE_RECORD_ONLY {
        return Ok(0);
    }

    // `on_date` 暂不参与计算（签名保留，供后续「按有效期折算可用」口径）
    let _ = on_date;
    // 纯查询走普通读：不加行锁，避免热点账户上无关读者互相排队
    let balance =
        time_off_repo::find_balance_by_account(txn, employee_id, time_off_type_id, period).await?;
    Ok(balance.map(|balance| available_of(&balance)).unwrap_or(0))
}

// —— 批量发放（service 编排：范围解析 + 逐人发放；测试要在 test_txn 里跑，故提供 _in_tx 变体）——

/// 批量发放额度（事务内实现）：范围解析 → 逐人调用 [`grant_time_off_in_tx`]。
///
/// 幂等：命中「员工 × 假别 × 依据 × 周期」的人计入 `skipped` 并记入 `skipped_employee_ids`
/// （**保持入参顺序**），供前端提示哪些人本周期已发放。
// 实现提示：① 范围解析（三选一，互斥性由 `validate.rs` 保证）——`req.all = true` → 全部在职员工
// （`hr_employee.employment_status != 3`，建议由 employee 域 repo 提供批量原语）；
// `req.dept_id = Some(id)` → 该部门挂载员工（`sys_user_dept`，建议由 dept 域 repo 提供）；
// 否则直接用 `req.employee_ids`（去重后按入参顺序处理）；② 解析 `req.effective_at` /
// `req.expire_at`（`chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")`，失败 →
// `AppError::Biz(format!("日期格式不正确：{s}"))`）；③ 逐人先
// `repo::find_grant_by_idempotent_key(txn, emp, req.time_off_type_id, &req.reason, &req.period)` 探测：
// 已存在 → `skipped += 1` + 记入 `skipped_employee_ids`；不存在 → `grant_time_off_in_tx(...)` 后
// `created += 1`；④ 返回 `BatchCreateGrantResp`。
// 骨架期：P2 请假 / P4 加班接入前无调用方（test 构建下由用例覆盖）
pub(crate) async fn batch_create_grants_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &BatchCreateGrantReq,
) -> Result<BatchCreateGrantResp, AppError> {
    let effective_at = parse_date(&req.effective_at)?;
    let expire_at = match req.expire_at.as_deref() {
        Some(raw) if !raw.trim().is_empty() => Some(parse_date(raw)?),
        _ => None,
    };
    let employees = resolve_grant_targets(txn, req).await?;

    let mut resp = BatchCreateGrantResp {
        created: 0,
        skipped: 0,
        skipped_employee_ids: Vec::new(),
    };
    for employee_id in employees {
        // 幂等：同「员工 × 假别 × 依据 × 周期」已发过 → 计入 skipped（保持入参顺序）
        let existing = time_off_repo::find_grant_by_idempotent_key(
            txn,
            employee_id,
            req.time_off_type_id,
            &req.reason,
            &req.period,
        )
        .await?;
        if existing.is_some() {
            resp.skipped += 1;
            resp.skipped_employee_ids.push(employee_id);
            continue;
        }

        grant_time_off_in_tx(
            txn,
            actor_id,
            employee_id,
            req.time_off_type_id,
            req.minutes,
            GRANT_SOURCE_ISSUE,
            &req.reason,
            &req.period,
            effective_at,
            expire_at,
        )
        .await?;
        resp.created += 1;
    }
    Ok(resp)
}

// —— 批量取名 helper（列表 / 详情响应用；单次查询，禁止逐行查库）——

/// 批量取员工名：`hr_employee.id` → `sys_user.username`（跨域读 employee 域 repo + user 域查名管道）。
/// 返回 id → 用户名映射；查不到的人（软删档案/用户）不出现，调用方留空串。
///
/// 与平台唯一拼名管道口径一致（映射 `sys_user.username`，employee 域 `EmployeeResp` 走同一条）；
/// 若将来要显示昵称，需另开设计（给 `utils::user_ref` 增加昵称口径），本域不自建查询。
// 实现提示：① 空入参直接返回空 map；② 按 ID 批量取档案（`ids` 去重后）——任务 4 在 employee 域
// 追加 `repo::find_by_ids(db, ids) -> anyhow::Result<Vec<hr_employee::Model>>`（本任务不代写）；
// ③ 取到的 `hr_employee.user_id` 交给全项目唯一查名管道
// `crate::utils::user_ref::find_user_name_map_by_ids`（映射 `sys_user.username`）；④ 回填
// `hr_employee.id → 名称`。禁止逐行查库：列表响应必须一次批量查询。
// 骨架期：任务 4 的 api 层接入后删除本属性
pub(crate) async fn fill_employee_names(
    db: &impl ConnectionTrait,
    employee_ids: &[u64],
) -> Result<HashMap<u64, String>, AppError> {
    let ids = crate::utils::user_ref::dedup_ids(employee_ids.to_vec());
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    // 一次批量取档案（拿 user_id）→ 一次批量查名 → 回填 employee_id → username
    let employees = crate::modules::biz::hr::employee::repo::find_by_ids(db, &ids).await?;
    let user_ids = employees.iter().map(|e| e.user_id).collect();
    let names = crate::utils::user_ref::find_user_name_map_by_ids(db, user_ids).await?;
    Ok(employees
        .into_iter()
        .filter_map(|e| names.get(&e.user_id).cloned().map(|name| (e.id, name)))
        .collect())
}

/// 批量取假期类型名：`hr_time_off_type.id` → `type_name`（单表批量查，排除软删行）。
// 实现提示：① 空入参返回空 map；② `repo::find_time_off_types_by_ids(db, ids)` 单表批量查
// （已按 `DeletedAt.is_null()` 排除软删；repo 是唯一数据访问层，本域不另写 SQL）；
// ③ 回填 `id → type_name`；软删行不出现，调用方留空串。
// 骨架期：任务 4 的 api 层接入后删除本属性
pub(crate) async fn fill_time_off_type_names(
    db: &impl ConnectionTrait,
    time_off_type_ids: &[u64],
) -> Result<HashMap<u64, String>, AppError> {
    let ids = crate::utils::user_ref::dedup_ids(time_off_type_ids.to_vec());
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    // 单表批量查（repo 已排除软删行；软删假别不出现在映射里，调用方留空串）
    let types = time_off_repo::find_time_off_types_by_ids(db, &ids).await?;
    Ok(types.into_iter().map(|t| (t.id, t.type_name)).collect())
}

// —— 资源 CRUD：只读入口直连 db，自持事务的写入口走「三行事务」——

/// 假期类型分页：请求参数组装为 repo 过滤条件后透传（keyword 同时模糊编码与名称）。
// 实现提示：`repo::find_time_off_type_page(db, &TimeOffTypeFilter { keyword: req.keyword.clone(),
// status: req.status }, req.page.page_index(), req.page.page_size())`；`*_by_name` 由 api 层经
// `utils::user_ref::fill_user_names` 拼装，本层只回 `Model`。
pub async fn page_time_off_types(
    db: &impl ConnectionTrait,
    req: &TimeOffTypeListReq,
) -> Result<PageData<hr_time_off_type::Model>, AppError> {
    let filter = TimeOffTypeFilter {
        keyword: req.keyword.clone(),
        status: req.status,
    };
    let page = time_off_repo::find_time_off_type_page(
        db,
        &filter,
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(page)
}

/// 创建假期类型（对外入口）：三行事务，成功后提交。
// 实现提示：`let txn = db.begin().await?;` → ① 编码查重
// `repo::find_time_off_type_by_code_include_deleted(&txn, &req.type_code)` 命中即
// `AppError::Biz(format!("类型编码已存在：{}", req.type_code))`（软删行仍占唯一键，必须查含软删）；
// ② 组装 `hr_time_off_type::ActiveModel`（业务列全量 Set）→
// `repo::create_time_off_type_in_tx(&txn, model, actor_id)`；③ `txn.commit().await?`
// （`?` 冒泡时事务 Drop 自动回滚，无需显式 rollback）；返回值原样回传。
pub async fn create_time_off_type(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateTimeOffTypeReq,
) -> Result<hr_time_off_type::Model, AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_time_off_type_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内创建假期类型：编码查重（**含软删占位**）→ 建行。
async fn create_time_off_type_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: CreateTimeOffTypeReq,
) -> Result<hr_time_off_type::Model, AppError> {
    // 唯一键是单列 `type_code`，软删行仍占位，所以查重必须看得见软删记录
    let duplicated =
        time_off_repo::find_time_off_type_by_code_include_deleted(txn, &req.type_code).await?;
    if duplicated.is_some() {
        return Err(AppError::Biz(format!("类型编码已存在：{}", req.type_code)));
    }

    let model = hr_time_off_type::ActiveModel {
        type_code: Set(req.type_code),
        type_name: Set(req.type_name),
        unit: Set(req.unit),
        balance_mode: Set(req.balance_mode),
        min_unit_minutes: Set(req.min_unit_minutes),
        require_attachment: Set(req.require_attachment),
        allow_negative: Set(req.allow_negative),
        pay_ratio: Set(req.pay_ratio),
        status: Set(req.status),
        remark: Set(req.remark),
        ..Default::default()
    };
    let created = time_off_repo::create_time_off_type_in_tx(txn, model, actor_id).await?;
    Ok(created)
}

/// 更新假期类型（对外入口）：三行事务，成功后提交。
// 实现提示：① `repo::find_time_off_type_by_id(&txn, req.id)` 判存在（软删视为不存在，
// 不存在 → `AppError::Biz(format!("假期类型不存在：{}", req.id))`）；② 编码查重**排除自身**
// （`find_time_off_type_by_code_include_deleted` 命中且 `id != req.id` 才报「类型编码已存在」）；
// ③ `repo::update_time_off_type_in_tx(&txn, ActiveModel { id: Set(req.id), ..业务列 Set }, actor_id)`
// （窄写，`created_by` 保持 NotSet）；④ commit。
pub async fn update_time_off_type(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateTimeOffTypeReq,
) -> Result<hr_time_off_type::Model, AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_time_off_type_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内更新假期类型：存在性 → 编码查重（排除自身）→ 窄写。
async fn update_time_off_type_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateTimeOffTypeReq,
) -> Result<hr_time_off_type::Model, AppError> {
    let existing = time_off_repo::find_time_off_type_by_id(txn, req.id).await?;
    if existing.is_none() {
        return Err(AppError::Biz(format!("假期类型不存在：{}", req.id)));
    }

    let duplicated =
        time_off_repo::find_time_off_type_by_code_include_deleted(txn, &req.type_code).await?;
    if duplicated.is_some_and(|model| model.id != req.id) {
        return Err(AppError::Biz(format!("类型编码已存在：{}", req.type_code)));
    }

    // 窄写：只 Set 业务列，`created_by` 等保持 NotSet 不被覆盖
    let model = hr_time_off_type::ActiveModel {
        id: Set(req.id),
        type_code: Set(req.type_code.clone()),
        type_name: Set(req.type_name.clone()),
        unit: Set(req.unit),
        balance_mode: Set(req.balance_mode),
        min_unit_minutes: Set(req.min_unit_minutes),
        require_attachment: Set(req.require_attachment),
        allow_negative: Set(req.allow_negative),
        pay_ratio: Set(req.pay_ratio),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };
    let updated = time_off_repo::update_time_off_type_in_tx(txn, model, actor_id).await?;
    Ok(updated)
}

/// 按 id 查假期类型详情（软删视为不存在）。
// 实现提示：`repo::find_time_off_type_by_id(db, id)` → `None` 即
// `AppError::Biz(format!("假期类型不存在：{id}"))`；人名由 api 层经 `fill_user_names` 拼装。
pub async fn get_time_off_type(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_time_off_type::Model, AppError> {
    let model = time_off_repo::find_time_off_type_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{id}")))?;
    Ok(model)
}

/// 删除假期类型（对外入口）：软删，三行事务，成功后提交。
// 实现提示：① 引用检查（是否已有批次挂在该假别上）——建议 `repo::find_grant_page(&txn,
// &TimeOffGrantFilter { time_off_type_id: Some(id), ..Default::default() }, 0, 1)` 非空即
// `AppError::Biz("该假期类型已有额度批次，不能删除")`（口径由产品定，也可改为允许删除）；
// ② `repo::soft_delete_time_off_type_in_tx(&txn, id, actor_id)` 返回 `false`（不存在 / 已软删）即
// `AppError::Biz(format!("假期类型不存在：{id}"))`；③ commit。
pub async fn delete_time_off_type(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_time_off_type_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内软删假期类型：先做引用检查（已挂批次不允许删）→ 软删。
async fn delete_time_off_type_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    // 引用检查：假别一旦发过额度就不能删（否则历史批次 / 余额会指向不存在的假别）
    let filter = TimeOffGrantFilter {
        time_off_type_id: Some(id),
        ..Default::default()
    };
    if !time_off_repo::find_grant_page(txn, &filter, 0, 1)
        .await?
        .items
        .is_empty()
    {
        return Err(AppError::Biz("该假期类型已有额度批次，不能删除".into()));
    }

    if !time_off_repo::soft_delete_time_off_type_in_tx(txn, id, actor_id).await? {
        return Err(AppError::Biz(format!("假期类型不存在：{id}")));
    }
    Ok(())
}

/// 额度批次分页：请求参数组装为 repo 过滤条件后透传。
// 实现提示：`repo::find_grant_page(db, &TimeOffGrantFilter { employee_id: req.employee_id,
// time_off_type_id: req.time_off_type_id, reason: req.reason.clone(), period: req.period.clone(),
// status: req.status }, req.page.page_index(), req.page.page_size())`；`employee_name` /
// `time_off_type_name` 由 api 层经 [`fill_employee_names`] / [`fill_time_off_type_names`] 批量回填。
pub async fn page_time_off_grants(
    db: &impl ConnectionTrait,
    req: &TimeOffGrantListReq,
) -> Result<PageData<hr_time_off_grant::Model>, AppError> {
    let filter = TimeOffGrantFilter {
        employee_id: req.employee_id,
        time_off_type_id: req.time_off_type_id,
        reason: req.reason.clone(),
        period: req.period.clone(),
        status: req.status,
    };
    let page =
        time_off_repo::find_grant_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?;
    Ok(page)
}

/// 批量发放额度（对外入口）：三行事务，成功后提交。
// 实现提示：`let txn = db.begin().await?;` → `batch_create_grants_in_tx(&txn, actor_id, req)` →
// 成功 `txn.commit().await?` → 结果原样回传（部分员工命中幂等不算失败，走 `skipped` 回执）。
pub async fn batch_create_grants(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &BatchCreateGrantReq,
) -> Result<BatchCreateGrantResp, AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = batch_create_grants_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 按 id 查额度批次详情。
// 实现提示：`repo::find_grant_by_id(db, id)` → `None` 即
// `AppError::Biz(format!("额度批次不存在：{id}"))`；人名回填同上。
pub async fn get_time_off_grant(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_time_off_grant::Model, AppError> {
    let model = time_off_repo::find_grant_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("额度批次不存在：{id}")))?;
    Ok(model)
}

/// 撤销额度批次（对外入口）：三行事务，成功后提交。
// 实现提示：① `repo::find_grant_by_id(&txn, id)` 判存在；② 已是终态
// （`status != GRANT_STATUS_ACTIVE`）→ `AppError::Biz("批次已用尽或已失效，不能撤销")`；
// ③ 账户回冲：账期取批次 `effective_at` 的自然年，`granted_minutes -= remaining_minutes`
// → `repo::update_balance_in_tx`；④ 批次 `remaining_minutes` 归零 +
// `repo::set_grant_status_in_tx(&txn, id, GRANT_STATUS_CANCELED, actor_id)`；
// ⑤ 写反向流水：`biz_type = LOG_BIZ_ADJUST`、`delta = −remaining`（可用减少，见文件头账本口径）、
// `grant_id = id`；⑥ commit。撤销后不变式 A / B 仍成立（`granted` 与批次剩余同减）。
pub async fn cancel_grant(db: &DatabaseConnection, actor_id: u64, id: u64) -> Result<(), AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = cancel_grant_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内撤销批次：账户 `granted` 回冲 + 批次剩余归零 + 反向流水（不变式 A / B 仍成立）。
async fn cancel_grant_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let grant = time_off_repo::find_grant_by_id(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("额度批次不存在：{id}")))?;
    if grant.status != GRANT_STATUS_ACTIVE {
        return Err(AppError::Biz("批次已用尽或已失效，不能撤销".into()));
    }

    // 账期取批次 `effective_at` 的自然年：与发放时入账的账户同桶
    let period = grant.effective_at.year().to_string();
    let balance =
        lock_balance_in_tx(txn, grant.employee_id, grant.time_off_type_id, &period).await?;

    let before = available_of(&balance);
    let remaining = grant.remaining_minutes;

    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            granted_minutes: Set(balance.granted_minutes - remaining),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    time_off_repo::consume_grant_in_tx(txn, grant.id, remaining, actor_id).await?;
    time_off_repo::set_grant_status_in_tx(txn, grant.id, GRANT_STATUS_CANCELED, actor_id).await?;

    append_balance_log_in_tx(
        txn,
        BalanceLog {
            balance_id: balance.id,
            employee_id: grant.employee_id,
            time_off_type_id: grant.time_off_type_id,
            grant_id: grant.id,
            biz_type: LOG_BIZ_ADJUST,
            before_minutes: before,
            after_minutes: before - i64::from(remaining),
            delta_minutes: -remaining,
            source_kind: LOG_SOURCE_MANUAL,
            source_id: 0,
            operator_id: actor_id,
            remark: "撤销发放",
        },
    )
    .await?;
    Ok(())
}

/// 额度账户分页：请求参数组装为 repo 过滤条件后透传。
// 实现提示：`repo::find_balance_page(db, &TimeOffBalanceFilter { employee_id: req.employee_id,
// time_off_type_id: req.time_off_type_id, period: req.period.clone() }, req.page.page_index(),
// req.page.page_size())`；人名由 api 层经 [`fill_employee_names`] / [`fill_time_off_type_names`] 回填。
pub async fn page_time_off_balances(
    db: &impl ConnectionTrait,
    req: &TimeOffBalanceListReq,
) -> Result<PageData<hr_time_off_balance::Model>, AppError> {
    let filter = TimeOffBalanceFilter {
        employee_id: req.employee_id,
        time_off_type_id: req.time_off_type_id,
        period: req.period.clone(),
    };
    let page =
        time_off_repo::find_balance_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?;
    Ok(page)
}

/// 按 id 查额度账户详情。
// 实现提示：`repo::find_balance_by_id(db, id)` → `None` 即
// `AppError::Biz(format!("额度账户不存在：{id}"))`；人名回填同上。
pub async fn get_time_off_balance(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_time_off_balance::Model, AppError> {
    let model = time_off_repo::find_balance_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("额度账户不存在：{id}")))?;
    Ok(model)
}

/// 额度流水分页：请求参数组装为 repo 过滤条件后透传（append-only 对账凭据）。
// 实现提示：`repo::find_balance_log_page(db, &TimeOffBalanceLogFilter { employee_id: req.employee_id,
// time_off_type_id: req.time_off_type_id, biz_type: req.biz_type }, req.page.page_index(),
// req.page.page_size())`；`employee_name` / `time_off_type_name` / `operator_name` 由 api 层回填
// （`operator_name` 走 `utils::user_ref` 唯一管道）。
pub async fn page_time_off_balance_logs(
    db: &impl ConnectionTrait,
    req: &TimeOffBalanceLogListReq,
) -> Result<PageData<hr_time_off_balance_log::Model>, AppError> {
    let filter = TimeOffBalanceLogFilter {
        employee_id: req.employee_id,
        time_off_type_id: req.time_off_type_id,
        biz_type: req.biz_type,
    };
    let page = time_off_repo::find_balance_log_page(
        db,
        &filter,
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::hr_employee;
    use crate::modules::biz::hr::time_off::dto::{BatchCreateGrantReq, TimeOffBalanceLogFilter};
    use crate::modules::biz::hr::time_off::{
        GRANT_SOURCE_ISSUE, GRANT_SOURCE_MANUAL, LOG_BIZ_GRANT, LOG_BIZ_TIME_OFF_RELEASE,
    };
    use crate::modules::biz::hr::time_off::{repo, service};
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同进程内并行用例必须互不相同，否则撞 `uk_hr_time_off_type_code`。
    ///
    /// 前缀用 `lt_service`：repo 测试模块用 `lt`，两个模块的 `SEQ` 各自从 0 起，
    /// 同进程并行跑全量测试时仅靠 `SEQ` 区分不开。
    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一员工 ID：段位 900_3xx，与 repo 测试（900_2xx）、employee 域测试（900_1xx）
    /// 错开，避免同进程并行撞 `uk_hr_employee_user_id`。
    fn unique_employee_id() -> u64 {
        900_300_000
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

        let mut model = time_off_type_model(unique("lt_service"));
        model.allow_negative = Set(allow_negative);
        let time_off_type = repo::create_time_off_type_in_tx(txn, model, ACTOR_ID)
            .await
            .unwrap();

        (employee.id, time_off_type.id)
    }

    #[tokio::test]
    async fn grant_time_off_creates_batch_and_account_and_log_in_one_call() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let grant_id = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            5 * 480,
            GRANT_SOURCE_MANUAL,
            "manual",
            "2026",
            date(2026, 1, 1),
            Some(date(2026, 12, 31)),
        )
        .await
        .unwrap();
        let balance = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            balance.granted_minutes,
            5 * 480,
            "发放后账户累计授予应为 5 天"
        );
        let logs = repo::find_balance_log_page(
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
        assert_eq!(
            logs.items[0].biz_type, LOG_BIZ_GRANT,
            "发放必须写一条授予流水"
        );
        assert_eq!(logs.items[0].grant_id, grant_id, "流水必须回指批次");
    }

    #[tokio::test]
    async fn grant_time_off_is_idempotent_for_same_reason_and_period() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let first = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            4800,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2026",
            date(2026, 1, 1),
            None,
        )
        .await
        .unwrap();
        let second = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            4800,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2026",
            date(2026, 1, 1),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            first, second,
            "同员工×假别×依据×周期重复发放必须复用同一批次"
        );
        let balance = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.granted_minutes, 4800, "重复发放不得翻倍累计");
    }

    #[tokio::test]
    async fn lock_then_consume_moves_locked_to_used_and_consumes_fefo_batch() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let expiring = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            1,
            "carry",
            "2025",
            date(2026, 1, 1),
            Some(date(2026, 3, 31)),
        )
        .await
        .unwrap();
        let _fresh = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            1,
            "statutory",
            "2026",
            date(2026, 1, 1),
            None,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(&txn, emp, ty, 480, date(2026, 2, 1))
            .await
            .unwrap();
        let locked = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(locked.locked_minutes, 480, "预占后锁定量应为 1 天");
        assert_eq!(locked.used_minutes, 0, "预占阶段不得计实扣");
        assert_eq!(
            locked.granted_minutes, 960,
            "临期批次也计入同一账期的累计授予（480+480）"
        );
        service::consume_locked_in_tx(&txn, emp, ty, 480, 2, 999, date(2026, 2, 1))
            .await
            .unwrap();
        let used = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(used.locked_minutes, 0, "实扣后锁定量必须归零");
        assert_eq!(used.used_minutes, 480, "实扣后累计实扣应为 1 天");
        let batch = repo::find_grant_by_id(&txn, expiring)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batch.remaining_minutes, 0, "FEFO 必须扣到临期批次");
        assert_eq!(batch.status, 2, "批次用尽后状态应为已用尽");
    }

    #[tokio::test]
    async fn lock_time_off_rejects_when_available_is_insufficient_and_type_disallows_negative() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type_with(&txn, 0 /* allow_negative */).await;
        // 先发一笔**不足额**的额度（240 < 480）：账户存在才可能走到「可用不足」分支，
        // 否则命中的是「账户不存在」文案，断言就失去区分力。
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            240,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2026",
            date(2026, 1, 1),
            None,
        )
        .await
        .unwrap();
        let err = service::lock_time_off_in_tx(&txn, emp, ty, 480, date(2026, 2, 1))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("额度不足"),
            "超额且不允许负数时必须报额度不足，实际：{err}"
        );
    }

    #[tokio::test]
    async fn release_locked_restores_available_and_writes_release_log() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type_with(&txn, 0).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2026",
            date(2026, 1, 1),
            None,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(&txn, emp, ty, 240, date(2026, 2, 1))
            .await
            .unwrap();
        service::release_locked_in_tx(&txn, emp, ty, 240, 2, 999, date(2026, 2, 1))
            .await
            .unwrap();

        let bal = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bal.locked_minutes, 0, "释放后锁定量必须归零");
        assert_eq!(bal.used_minutes, 0, "释放不得计入实扣");
        assert_eq!(
            service::available_minutes(&txn, emp, ty, "2026", date(2026, 2, 1))
                .await
                .unwrap(),
            480,
            "释放后可用额度必须恢复到 1 天"
        );
        let logs = repo::find_balance_log_page(
            &txn,
            &TimeOffBalanceLogFilter {
                employee_id: Some(emp),
                biz_type: Some(LOG_BIZ_TIME_OFF_RELEASE),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(logs.items.len(), 1, "释放必须恰好写一条释放流水");
        // 释放流水记的是**可用恢复量**（after − before = +240，见 service.rs 文件头账本口径）；
        // locked / used 口径由不变式 A 逐行钉死。
        assert_eq!(
            logs.items[0].delta_minutes, 240,
            "释放流水必须是正向的可用恢复量"
        );
    }

    #[tokio::test]
    async fn expire_grants_zeroes_outdated_batches_and_is_idempotent() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let stale = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            1,
            "carry",
            "2025",
            date(2026, 1, 1),
            Some(date(2026, 3, 31)),
        )
        .await
        .unwrap();
        let done = service::expire_grants_in_tx(&txn, date(2026, 4, 1))
            .await
            .unwrap();
        assert!(done >= 1, "过期批次必须被作废，实际作废 {done} 条");
        let batch = repo::find_grant_by_id(&txn, stale).await.unwrap().unwrap();
        assert_eq!(batch.remaining_minutes, 0, "过期后剩余必须归零");
        assert_eq!(batch.status, 3, "过期后状态应为已失效");
        let balance = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            balance.expired_minutes, 480,
            "过期必须计入账户失效口径（账期与发放同口径：批次生效日的自然年）"
        );
        let again = service::expire_grants_in_tx(&txn, date(2026, 4, 1))
            .await
            .unwrap();
        assert!(
            repo::find_grant_by_id(&txn, stale)
                .await
                .unwrap()
                .unwrap()
                .status
                == 3
                && again <= done,
            "重复作废必须幂等"
        );
    }

    /// 跨年作废的入账口径：`expire` 必须回到**批次发放时入账的那个年度账户**，
    /// 不能按 `today` 建/记另一个年度的桶（否则批次被清零而原授予桶仍留可用量，不变式破）。
    #[tokio::test]
    async fn expire_grants_books_to_the_same_account_as_the_grant_across_years() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        // 2025 年生效、2025 年底失效的当年既发即失效批次 → 入 2025 账期账户
        let _stale = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            GRANT_SOURCE_ISSUE,
            "carry",
            "2025",
            date(2025, 1, 1),
            Some(date(2025, 12, 31)),
        )
        .await
        .unwrap();

        service::expire_grants_in_tx(&txn, date(2026, 4, 1))
            .await
            .unwrap();

        let bal_2025 = repo::find_balance_by_account_for_update(&txn, emp, ty, "2025")
            .await
            .unwrap()
            .expect("2025 年账户必须存在");
        assert_eq!(
            bal_2025.expired_minutes, 480,
            "作废必须记在批次入账的同一年度账户（2025）"
        );
        assert!(
            repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
                .await
                .unwrap()
                .is_none(),
            "跨年作废不得在 2026 年凭空建桶"
        );
    }

    #[tokio::test]
    async fn ledger_invariants_hold_after_grant_lock_consume() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type_with(&txn, 0).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            960,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2026",
            date(2026, 1, 1),
            None,
        )
        .await
        .unwrap();
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            GRANT_SOURCE_ISSUE,
            "carry",
            "2026",
            date(2026, 1, 1),
            Some(date(2026, 6, 30)),
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(&txn, emp, ty, 480, date(2026, 2, 1))
            .await
            .unwrap();
        service::consume_locked_in_tx(&txn, emp, ty, 480, 2, 999, date(2026, 2, 1))
            .await
            .unwrap();

        let bal = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        let logs = repo::find_balance_log_page(
            &txn,
            &TimeOffBalanceLogFilter {
                employee_id: Some(emp),
                ..Default::default()
            },
            0,
            100,
        )
        .await
        .unwrap();
        assert!(
            logs.items
                .iter()
                .all(|l| l.delta_minutes == l.after_minutes - l.before_minutes),
            "每条流水的 delta 必须等于可用余额变动量（after − before）"
        );
        let delta_sum: i64 = logs.items.iter().map(|l| i64::from(l.delta_minutes)).sum();
        let account_net = i64::from(bal.granted_minutes) + i64::from(bal.adjust_minutes)
            - i64::from(bal.used_minutes)
            - i64::from(bal.locked_minutes)
            - i64::from(bal.expired_minutes);
        assert_eq!(delta_sum, account_net, "不变式 A：流水净额必须等于账户净额");

        let grants = repo::find_active_grants_for_update(&txn, emp, ty, date(2026, 2, 1))
            .await
            .unwrap();
        let remaining_sum: i64 = grants.iter().map(|g| i64::from(g.remaining_minutes)).sum();
        assert_eq!(
            remaining_sum,
            i64::from(bal.granted_minutes) - i64::from(bal.used_minutes),
            "不变式 B：未失效批次的剩余合计必须等于累计授予减实扣（含 FEFO 扣到临期批次）"
        );
    }

    #[tokio::test]
    async fn batch_create_grants_skips_employees_already_granted_for_period() {
        let txn = test_txn().await;
        let (emp1, ty) = seed_employee_and_type(&txn).await;
        let (emp2, _ty2) = seed_employee_and_type(&txn).await;
        let req = BatchCreateGrantReq {
            employee_ids: vec![emp1, emp2],
            dept_id: None,
            all: false,
            time_off_type_id: ty,
            minutes: 4800,
            reason: "statutory".to_string(),
            period: "2026".to_string(),
            effective_at: "2026-01-01".to_string(),
            expire_at: None,
            remark: String::new(),
        };

        let first = service::batch_create_grants_in_tx(&txn, ACTOR_ID, &req)
            .await
            .unwrap();
        assert_eq!(
            (first.created, first.skipped),
            (2, 0),
            "首次发放应两人各建一个批次"
        );
        let second = service::batch_create_grants_in_tx(&txn, ACTOR_ID, &req)
            .await
            .unwrap();
        assert_eq!(
            (second.created, second.skipped),
            (0, 2),
            "同周期重复发放应全部跳过（幂等）"
        );
        assert_eq!(
            second.skipped_employee_ids,
            vec![emp1, emp2],
            "跳过名单必须按入参顺序回传"
        );

        let bal = repo::find_balance_by_account_for_update(&txn, emp1, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bal.granted_minutes, 4800, "重复发放不得翻倍累计");
    }
}
