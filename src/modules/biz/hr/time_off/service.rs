//! 假期额度域业务：额度原语（发放 / 预占 / 实扣 / 释放 / 过期 / 可用查询）+ 资源 CRUD 编排。
//!
//! 分层约定（见 AGENTS.md「分层契约」）：
//! - 自持事务的入口收 `db: &DatabaseConnection`，内部 `begin` → 委托
//!   `pub(crate) *_in_tx(txn, …)` → 成功即 `commit`；`*_in_tx` 内**不得** begin / commit
//!   （事务边界由入口与测试外层事务负责）；
//! - 只读入口收 `&impl ConnectionTrait`，不起事务；
//! - 额度原语是请假 / 加班的**共用写入口**：账本两条不变式由本层维护——
//!   A：`Σ log.delta_minutes == granted + adjust − used − locked − expired`；
//!   B：`Σ 未失效批次 remaining + locked == granted + adjust − used − expired`；
//!   （`locked` 必须进 B 的左边：预占即扣批次后，在途量已从批次 `remaining` 移走，
//!   见下文「预占即扣批次」；P0–P1 的旧表述 `Σ remaining == granted − used` 在本口径下必然破）
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
//! # 预占即扣批次（P2 口径修订）
//!
//! `lock_time_off_in_tx` 在加账户 `locked` 的同时**按 FEFO 直接扣批次 `remaining`**，并每批写一条
//! 带 `grant_id` 的预占流水；`consume_locked_in_tx` 只做账户 `locked → used` 迁移（批次不再扣，
//! 避免重复计数）；`release_locked_in_tx` 按预占流水把量还回原批次，原批次已失效 / 已撤销时
//! 还到当前账期的「归还批次」（同一单据复用同一个批次，幂等键含 `source_kind = 2 请假单 + source_id`）。
//!
//! 为什么必须这样：过期 job 只看批次 `remaining`。预占若不动批次，跨期过期会把在途量一并作废
//! （job 看不到它），驳回释放再把它加回来 —— 可用额度会凭空多出甚至变成负数。
//!
//! 单据生命周期内的批次归属以**预占流水**为准（`(source_kind, source_id)` 回读），
//! 实扣 / 释放都不重新按 FEFO 现算，避免归属漂移。
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
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::{
    hr_employee, hr_time_off_balance, hr_time_off_balance_log, hr_time_off_grant,
    hr_time_off_request, hr_time_off_type,
};
use crate::modules::biz::hr::employee::EMPLOYMENT_STATUS_RESIGNED;
use crate::modules::biz::hr::time_off::dto::{
    BatchCreateGrantReq, BatchCreateGrantResp, CreateTimeOffRequestReq, CreateTimeOffTypeReq,
    MineTimeOffRequestReq, TimeOffBalanceFilter, TimeOffBalanceListReq, TimeOffBalanceLogFilter,
    TimeOffBalanceLogListReq, TimeOffGrantFilter, TimeOffGrantListReq, TimeOffRequestFilter,
    TimeOffRequestListReq, TimeOffTypeFilter, TimeOffTypeListReq, UpdateTimeOffRequestReq,
    UpdateTimeOffTypeReq,
};
use crate::modules::biz::hr::time_off::{
    BALANCE_MODE_RECORD_ONLY, GRANT_REASON_RELEASE_RESTORE, GRANT_SOURCE_ISSUE,
    GRANT_SOURCE_MANUAL, GRANT_STATUS_ACTIVE, GRANT_STATUS_CANCELED, GRANT_STATUS_EXHAUSTED,
    GRANT_STATUS_EXPIRED, LOG_BIZ_ADJUST, LOG_BIZ_EXPIRE, LOG_BIZ_GRANT, LOG_BIZ_TIME_OFF_CONSUME,
    LOG_BIZ_TIME_OFF_LOCK, LOG_BIZ_TIME_OFF_RELEASE, REQUEST_STATUS_APPROVED,
    REQUEST_STATUS_CANCELED, REQUEST_STATUS_PENDING, REQUEST_STATUS_REJECTED, SOURCE_KIND_JOB,
    SOURCE_KIND_MANUAL, SOURCE_KIND_TIME_OFF, TIME_OFF_TYPE_ENABLED, repo as time_off_repo,
};
use crate::utils::PageData;
use crate::utils::error::AppError;
use chrono::Datelike;

// —— 额度原语（pub(crate)：P2 请假 / P4 加班复用；调用方必须自持事务）——

/// 发放额度：幂等键（员工 × 假别 × 依据 × 周期）命中即复用，否则建批次 + 记账户 + 写流水。
///
/// 返回批次 ID。调用方（[`batch_create_grants_in_tx`] / 请假单归还 / 加班转调休）负责事务边界。
///
/// 幂等键：`source_id != 0`（单据来源：加班单、请假单归还）用「员工 × 假别 × source_kind × source_id」
/// 键，否则用「员工 × 假别 × 依据 × 周期」键——后者区分不了同一年第二次加班入账，会把第二次入账吞掉。
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
    source_kind: i8,
    source_id: u64,
) -> Result<u64, AppError> {
    // ① 幂等：`source_id != 0` 用来源键（单据来源），否则用 reason + period
    let existing = if source_id != 0 {
        time_off_repo::find_grant_by_source_key(
            txn,
            employee_id,
            time_off_type_id,
            source_kind,
            source_id,
        )
        .await?
    } else {
        time_off_repo::find_grant_by_idempotent_key(
            txn,
            employee_id,
            time_off_type_id,
            reason,
            period,
        )
        .await?
    };
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
            source_kind: Set(source_kind),
            source_id: Set(source_id),
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
            source_kind,
            source_id,
            operator_id: actor_id,
            remark: "",
        },
    )
    .await?;

    Ok(grant.id)
}

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

/// 预占额度（审批中）：账户 `locked_minutes += minutes`，并**按 FEFO 直接扣批次剩余**。
///
/// 为什么预占要扣批次（P0–P1 遗留口径 #1）：过期 job 按 `hr_time_off_grant.remaining_minutes`
/// 作废，若预占只加账户 `locked`，在途预占跨过批次失效日时会被 job 一并作废（job 看不到它），
/// 随后驳回释放又把可用额度加回来 —— 账本会出现凭空多出的额度、甚至负余额。预占即扣批次后，
/// 在途量已移出 `remaining`，job 天然看不到它，本问题从根上消失。
///
/// 批次归属靠流水留痕：每扣一批写一条 `biz_type = 3` 的流水（`grant_id` = 该批、
/// `delta = −该批扣减量`），[`consume_locked_in_tx`] / [`release_locked_in_tx`] 按
/// `(source_kind, source_id)` 回读这些流水结算，**不再重新按 FEFO 现算**（否则批次归属会漂移）。
/// 预占**不翻转批次 `status`**：剩余归零可能只是被预占，驳回还要还回去。
///
/// `source_id` 记**审批实例 ID**（= 一次提交周期），不是请假单 ID：同一单据允许
/// 「驳回 → 修改 → 重新提交」，用单据 ID 作来源会让两轮预占串在一本账上
/// （第二轮释放被幂等守卫跳过、第二轮实扣按两轮求和判定「预占量不足」）。
///
/// `allow_negative = 1` 时批次覆盖不足的缺口写一条 `grant_id = 0` 的账户级流水
/// （缺口不属于任何批次），账户照常 `locked += minutes`，可用余额允许为负。
// 成对原语的入参集合固定；拆参数结构体会让调用方多一层构造，收益不足
#[allow(clippy::too_many_arguments)]
pub(crate) async fn lock_time_off_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
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

    // FEFO 扣批次：每批一条预占流水，`before/after` 按扣减顺序推进（delta == after − before）
    let batches =
        time_off_repo::find_active_grants_for_update(txn, employee_id, time_off_type_id, on_date)
            .await?;
    let mut need = minutes;
    let mut covered = 0i32;
    for batch in batches {
        if need == 0 {
            break;
        }
        let take = need.min(batch.remaining_minutes);
        if take <= 0 {
            continue;
        }
        // 批次级并发护栏在 repo 的 `remaining >= minutes` 条件里；false = 已被并发扣空
        if !time_off_repo::consume_grant_in_tx(txn, batch.id, take, actor_id).await? {
            return Err(AppError::Biz("有效额度批次不足，请重试".into()));
        }
        append_balance_log_in_tx(
            txn,
            BalanceLog {
                balance_id: balance.id,
                employee_id,
                time_off_type_id,
                grant_id: batch.id,
                biz_type: LOG_BIZ_TIME_OFF_LOCK,
                before_minutes: before - i64::from(covered),
                after_minutes: before - i64::from(covered) - i64::from(take),
                delta_minutes: -take,
                source_kind,
                source_id,
                operator_id: actor_id,
                remark: "",
            },
        )
        .await?;
        covered += take;
        need -= take;
    }

    if need > 0 {
        if type_model.allow_negative == 0 {
            // 账户可用额度可能包含「已过 expire_at 但 job 尚未作废」的批次（job 每日 01:30 跑），
            // 此时 FEFO 取不到它，如实报「有效批次」而不是谎称账本不一致
            return Err(AppError::Biz(format!(
                "有效额度批次不足：已覆盖 {covered} 分钟，本次需要 {minutes} 分钟"
            )));
        }
        append_balance_log_in_tx(
            txn,
            BalanceLog {
                balance_id: balance.id,
                employee_id,
                time_off_type_id,
                grant_id: 0, // 负余额缺口：不属于任何批次
                biz_type: LOG_BIZ_TIME_OFF_LOCK,
                before_minutes: before - i64::from(covered),
                after_minutes: before - i64::from(minutes),
                delta_minutes: -need,
                source_kind,
                source_id,
                operator_id: actor_id,
                remark: "负余额缺口",
            },
        )
        .await?;
    }

    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            locked_minutes: Set(balance.locked_minutes + minutes),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    Ok(())
}

/// 实扣（审批通过）：把该单据的预占转为实扣，账户 `locked -= m`、`used += m`（可用不变）。
///
/// 批次侧**不再扣减**（预占时已扣，见 [`lock_time_off_in_tx`]）：按 `(source_kind, source_id)`
/// 回读预占流水拿「这一轮提交动了哪些批次、各自动了多少」（`source_id` = 审批实例 ID，
/// 因此只会读到**本轮**的预占），据此
/// ① 批次剩余归零且仍在效时置 `EXHAUSTED`；
/// ② 每批补一条 `delta = 0` 的实扣流水（`grant_id` 指向该批，保留「这笔假扣的是哪一批」的追溯，
/// 也用 0 而非 `−m` 避免与预占的 `−m` 重复计数）。
pub(crate) async fn consume_locked_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    employee_id: u64,
    time_off_type_id: u64,
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

    // 预占量以流水为准（调用方不传 minutes，避免与账本漂移）
    let locks =
        time_off_repo::find_logs_by_source(txn, source_kind, source_id, LOG_BIZ_TIME_OFF_LOCK)
            .await?;
    if locks.is_empty() {
        return Err(AppError::Biz("该单据没有预占记录，请检查账本一致性".into()));
    }
    let minutes: i32 = locks.iter().map(|log| -log.delta_minutes).sum();
    if minutes <= 0 {
        return Err(AppError::Biz("预占流水异常，请检查账本一致性".into()));
    }
    if i64::from(balance.locked_minutes) < i64::from(minutes) {
        return Err(AppError::Biz("预占量不足，请检查账本一致性".into()));
    }

    let current = available_of(&balance);
    for log in &locks {
        if log.grant_id != 0
            && let Some(batch) =
                time_off_repo::find_grant_by_id_for_update(txn, log.grant_id).await?
            && batch.remaining_minutes == 0
            && batch.status == GRANT_STATUS_ACTIVE
        {
            time_off_repo::set_grant_status_in_tx(txn, batch.id, GRANT_STATUS_EXHAUSTED, actor_id)
                .await?;
        }
        append_balance_log_in_tx(
            txn,
            BalanceLog {
                balance_id: balance.id,
                employee_id,
                time_off_type_id,
                grant_id: log.grant_id,
                biz_type: LOG_BIZ_TIME_OFF_CONSUME,
                before_minutes: current,
                after_minutes: current,
                delta_minutes: 0,
                source_kind,
                source_id,
                operator_id: actor_id,
                remark: "",
            },
        )
        .await?;
    }

    // locked → used：可用值不变（见文件头账本口径）
    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            locked_minutes: Set(balance.locked_minutes - minutes),
            used_minutes: Set(balance.used_minutes + minutes),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    Ok(())
}

/// 释放预占（审批驳回 / 撤销）：账户 `locked -= m`，额度**归还原批次**；原批次已失效或已撤销时
/// 归还到当前账期（`on_date` 自然年）的归还批次。
///
/// 为什么允许归还到当前账期：预占时已把量从批次 `remaining` 移走，若原批次在审批期间过期作废，
/// 那份额度**没有**被计进 `expired`（job 看不到在途量），驳回归还它在账上仍然成立
/// （不变式 B 的 `+locked` 项对冲）。归还批次不动账户 `granted`：额度不是新授予，
/// 只是把原本就授予过的那份还回可用。
///
/// 幂等：同一 `(source_kind, source_id)` 已有释放流水则直接返回——重复驳回 / 重复撤销不会二次归还。
/// `source_id` = 审批实例 ID（一次提交周期）：同一单据重提后再驳回是**另一个来源**，
/// 因此第二轮照常归还（用单据 ID 会让第二轮归还被这条守卫静默跳过，额度永久滞留 `locked`）。
pub(crate) async fn release_locked_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    employee_id: u64,
    time_off_type_id: u64,
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

    let locks =
        time_off_repo::find_logs_by_source(txn, source_kind, source_id, LOG_BIZ_TIME_OFF_LOCK)
            .await?;
    if locks.is_empty() {
        // 没有预占（如记录型假别、或额度链路整体跳过）→ 无事可做
        return Ok(());
    }
    let already_released =
        time_off_repo::find_logs_by_source(txn, source_kind, source_id, LOG_BIZ_TIME_OFF_RELEASE)
            .await?;
    if !already_released.is_empty() {
        return Ok(());
    }

    let period = on_date.year().to_string();
    let balance = lock_balance_in_tx(txn, employee_id, time_off_type_id, &period).await?;

    let minutes: i32 = locks.iter().map(|log| -log.delta_minutes).sum();
    if minutes <= 0 {
        return Err(AppError::Biz("预占流水异常，请检查账本一致性".into()));
    }
    if i64::from(balance.locked_minutes) < i64::from(minutes) {
        return Err(AppError::Biz("释放量超过预占量，请检查账本一致性".into()));
    }

    let mut before = available_of(&balance);
    for log in &locks {
        let amount = -log.delta_minutes;
        if amount <= 0 {
            continue;
        }

        if log.grant_id == 0 {
            // 负余额缺口：归还即冲减负余额，没有批次可加
            append_balance_log_in_tx(
                txn,
                BalanceLog {
                    balance_id: balance.id,
                    employee_id,
                    time_off_type_id,
                    grant_id: 0,
                    biz_type: LOG_BIZ_TIME_OFF_RELEASE,
                    before_minutes: before,
                    after_minutes: before + i64::from(amount),
                    delta_minutes: amount,
                    source_kind,
                    source_id,
                    operator_id: actor_id,
                    remark: "负余额缺口归还",
                },
            )
            .await?;
            before += i64::from(amount);
            continue;
        }

        let batch = time_off_repo::find_grant_by_id_for_update(txn, log.grant_id).await?;
        let restorable = matches!(
            &batch,
            Some(b) if b.status == GRANT_STATUS_ACTIVE
                && b.expire_at.is_none_or(|expire_at| expire_at >= on_date)
        );
        // 归还流水的 `grant_id` 指向**实际承载归还量的批次**：能回原批就写原批，
        // 否则写归还批次（按流水排查「这笔归还进了哪个桶」才不会指错）
        let target_grant_id = if restorable {
            time_off_repo::add_grant_remaining_in_tx(txn, log.grant_id, amount, actor_id).await?;
            log.grant_id
        } else {
            // 原批次已失效 / 已撤销 / 已用尽：归还到当前账期的归还批次（同一提交周期复用同一个）
            let restore_id = find_or_create_restore_grant_in_tx(
                txn,
                actor_id,
                employee_id,
                time_off_type_id,
                source_id,
                on_date,
            )
            .await?;
            time_off_repo::add_grant_remaining_in_tx(txn, restore_id, amount, actor_id).await?;
            restore_id
        };

        append_balance_log_in_tx(
            txn,
            BalanceLog {
                balance_id: balance.id,
                employee_id,
                time_off_type_id,
                grant_id: target_grant_id,
                biz_type: LOG_BIZ_TIME_OFF_RELEASE,
                before_minutes: before,
                after_minutes: before + i64::from(amount),
                delta_minutes: amount,
                source_kind,
                source_id,
                operator_id: actor_id,
                remark: "",
            },
        )
        .await?;
        before += i64::from(amount);
    }

    time_off_repo::update_balance_in_tx(
        txn,
        hr_time_off_balance::ActiveModel {
            id: Set(balance.id),
            locked_minutes: Set(balance.locked_minutes - minutes),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    Ok(())
}

/// 取 / 建「归还批次」：承载驳回释放归还量的批次（当前账期，`expire_at` 取该年 12-31）。
///
/// 幂等键 = `(员工 × 假别 × 请假单来源 × 请假单 ID)`（`source_kind = SOURCE_KIND_TIME_OFF`）：
/// 同一张单据的多次归还都落到同一个批次，重复执行不会增殖批次。
///
/// 注意 `minutes` 列只记首次归还量：它不参与账户守恒（账户的 `granted` 在**原始批次**上
/// 已经计过，归还不是新授予），只作展示。
async fn find_or_create_restore_grant_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    employee_id: u64,
    time_off_type_id: u64,
    source_id: u64,
    on_date: Date,
) -> Result<u64, AppError> {
    if let Some(existing) = time_off_repo::find_grant_by_source_key(
        txn,
        employee_id,
        time_off_type_id,
        SOURCE_KIND_TIME_OFF,
        source_id,
    )
    .await?
    {
        return Ok(existing.id);
    }

    let year = on_date.year();
    let grant = time_off_repo::create_grant_in_tx(
        txn,
        hr_time_off_grant::ActiveModel {
            employee_id: Set(employee_id),
            time_off_type_id: Set(time_off_type_id),
            source: Set(GRANT_SOURCE_MANUAL),
            reason: Set(GRANT_REASON_RELEASE_RESTORE.to_string()),
            source_kind: Set(SOURCE_KIND_TIME_OFF),
            source_id: Set(source_id),
            period: Set(year.to_string()),
            minutes: Set(0),
            remaining_minutes: Set(0),
            effective_at: Set(on_date),
            expire_at: Set(chrono::NaiveDate::from_ymd_opt(year, 12, 31)),
            status: Set(GRANT_STATUS_ACTIVE),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(grant.id)
}

/// 批量作废过期批次（定时任务调用）：`expire_at < today` 且仍有剩余的批次归零 + 置失效 + 记流水。
///
/// 返回本次作废的批次数。幂等靠 `remaining_minutes > 0` 的扫描条件——重复跑不会二次记账。
pub(crate) async fn expire_grants_in_tx(
    txn: &DatabaseTransaction,
    today: Date,
) -> Result<u64, AppError> {
    let candidates = time_off_repo::find_expired_grants(txn, today).await?;
    let mut expired_count = 0u64;

    for candidate in candidates {
        // 锁序统一「账户 → 批次」（与 lock / consume / release 一致）：反序会与并发的请假提交
        // 构成 ABBA 环路，被 MySQL 以 1213 杀掉其中一个事务。
        //
        // 账期取批次 `effective_at` 的自然年：必须与它发放时入账的账户同桶（跨年作废不拆桶）。
        // `effective_at` 建行后不可变，因此用候选行的普通读值算账期是安全的。
        let period = candidate.effective_at.year().to_string();
        let balance = lock_balance_in_tx(
            txn,
            candidate.employee_id,
            candidate.time_off_type_id,
            &period,
        )
        .await
        .map_err(|_| AppError::Biz(format!("额度账户不存在，账本异常：批次 {}", candidate.id)))?;

        // 拿到账户锁之后再锁批次并重判：并发执行（或重复执行）下已被处理的批次直接跳过
        let Some(batch) = time_off_repo::find_grant_by_id_for_update(txn, candidate.id).await?
        else {
            continue;
        };
        if batch.status != GRANT_STATUS_ACTIVE || batch.remaining_minutes <= 0 {
            continue;
        }

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
                source_kind: SOURCE_KIND_JOB,
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
            SOURCE_KIND_MANUAL,
            0,
        )
        .await?;
        resp.created += 1;
    }
    Ok(resp)
}

// —— 批量取名 helper（列表 / 详情响应用；单次查询，禁止逐行查库）——

/// 批量取假期类型名：`hr_time_off_type.id` → `type_name`（单表批量查，排除软删行）。
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
            source_kind: SOURCE_KIND_MANUAL,
            source_id: 0,
            operator_id: actor_id,
            remark: "撤销发放",
        },
    )
    .await?;
    Ok(())
}

/// 额度账户分页：请求参数组装为 repo 过滤条件后透传。
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

// —— 请假单（P2）：时长派生 · 提交（预占 + 起审批）· 撤销 / 删除 / 查询 ——

/// 请假时长派生：按「排班 × 工作日历」逐日折算 `[start_at, end_at)` 内的应出勤分钟。
///
/// 口径见考勤域 `derive_work_minutes`（本函数只负责把 0 当成业务错误抛出：
/// 「选了整段休息时间」在请假场景下没有意义，静默建 0 分钟单据是更坏的结果）。
async fn require_duration_minutes(
    db: &impl ConnectionTrait,
    employee_id: u64,
    start_at: DateTime,
    end_at: DateTime,
) -> Result<i32, AppError> {
    let minutes = crate::modules::biz::hr::attendance::service::derive_work_minutes(
        db,
        employee_id,
        start_at,
        end_at,
    )
    .await?;
    if minutes <= 0 {
        return Err(AppError::Biz(
            "所选区间内没有应出勤的工作时间，请检查起止时间".into(),
        ));
    }
    Ok(minutes)
}

/// 取当前登录用户对应的员工档案（「我的请假单」用）；没有档案即报错。
async fn require_my_employee(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> Result<hr_employee::Model, AppError> {
    time_off_repo::find_employee_by_user_id(db, user_id)
        .await?
        .ok_or_else(|| AppError::Biz("未找到当前用户的员工档案".into()))
}

/// 校验请假单属于当前操作人（只有本人能撤销 / 删除自己的单据）。
async fn ensure_request_owner(
    db: &impl ConnectionTrait,
    actor_id: u64,
    request: &hr_time_off_request::Model,
) -> Result<(), AppError> {
    let employee = time_off_repo::find_employee_by_id(db, request.employee_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("员工档案不存在：{}", request.employee_id)))?;
    if employee.user_id != actor_id {
        return Err(AppError::Biz("只能操作本人的请假单".into()));
    }
    Ok(())
}

/// 提交请假单（事务内实现，`create` 与 `submit` 共用）：
/// 员工行锁 → 假别 / 员工校验 → 区间不重叠 → 后端派生时长 → 预占额度 → 起审批实例 → 落状态。
///
/// **员工行锁**是这条链的并发护栏：区间重叠是跨假别的「读 → 判断 → 写」，账户行锁只能串行化
/// 同假别，故按员工行串行化提交。
pub(crate) async fn submit_time_off_request_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<hr_time_off_request::Model, AppError> {
    let request = time_off_repo::find_request_by_id_for_update(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("请假单不存在：{id}")))?;
    ensure_request_owner(txn, actor_id, &request).await?;
    if !matches!(
        request.status,
        REQUEST_STATUS_REJECTED | REQUEST_STATUS_CANCELED
    ) {
        return Err(AppError::Biz(
            "只有已驳回或已撤销的请假单可以重新提交".into(),
        ));
    }

    let employee = time_off_repo::lock_employee_for_update(txn, request.employee_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("员工档案不存在：{}", request.employee_id)))?;
    if employee.employment_status == EMPLOYMENT_STATUS_RESIGNED {
        return Err(AppError::Biz("该员工已离职，不能提交请假单".into()));
    }

    let type_model = time_off_repo::find_time_off_type_by_id(txn, request.time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{}", request.time_off_type_id)))?;
    if type_model.status != TIME_OFF_TYPE_ENABLED {
        return Err(AppError::Biz(format!(
            "假期类型已停用，不能提交：{}",
            type_model.type_name
        )));
    }
    if type_model.require_attachment == 1 && request.attachment_id == 0 {
        return Err(AppError::Biz(format!(
            "假期类型「{}」必须上传附件",
            type_model.type_name
        )));
    }

    let overlaps = time_off_repo::find_overlapping_requests(
        txn,
        request.employee_id,
        request.id,
        request.start_at,
        request.end_at,
    )
    .await?;
    if let Some(first) = overlaps.first() {
        return Err(AppError::Biz(format!(
            "该时间段与已有请假单重叠（单据编号：{}）",
            first.id
        )));
    }

    // 时长由后端派生（请求体不接受 durationMinutes，杜绝前端伪造）
    let minutes =
        require_duration_minutes(txn, request.employee_id, request.start_at, request.end_at)
            .await?;

    // 先起审批实例，再预占额度：预占流水的来源键就是**这一次提交周期**（实例 ID）。
    // 若用单据 ID 作来源，同一单据「驳回 → 改 → 重提」的两轮预占会串在一本账上
    // （第二轮释放被「已有释放流水」跳过、第二轮实扣按两轮求和判定预占不足）。
    let instance_id = crate::modules::biz::hr::approval::service::start_instance_in_tx(
        txn,
        actor_id,
        crate::modules::biz::hr::approval::BIZ_TYPE_TIME_OFF,
        request.id,
        employee.user_id,
    )
    .await?;

    // 预占：按 FEFO 直接扣批次（记录型假别在本层 no-op）
    lock_time_off_in_tx(
        txn,
        actor_id,
        request.employee_id,
        request.time_off_type_id,
        minutes,
        SOURCE_KIND_TIME_OFF,
        instance_id,
        request.start_at.date(),
    )
    .await?;

    Ok(time_off_repo::update_request_in_tx(
        txn,
        hr_time_off_request::ActiveModel {
            id: Set(request.id),
            duration_minutes: Set(minutes),
            status: Set(REQUEST_STATUS_PENDING),
            approval_instance_id: Set(instance_id),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 事务内创建请假单并**直接提交**（建单即提交，无草稿态）。
pub(crate) async fn create_time_off_request_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateTimeOffRequestReq,
) -> Result<hr_time_off_request::Model, AppError> {
    let start_at = parse_request_datetime("请假开始时间", &req.start_at)?;
    let end_at = parse_request_datetime("请假结束时间", &req.end_at)?;

    let employee = time_off_repo::lock_employee_for_update(txn, req.employee_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("员工档案不存在：{}", req.employee_id)))?;
    // 写入口只服务本人：`employee_id` 来自请求体，不校验就能替他人建单并占他人额度
    // （HR 的批量事实录入走考勤域的 import，不做代报请假）
    if employee.user_id != actor_id {
        return Err(AppError::Biz("只能为本人创建请假单".into()));
    }
    if employee.employment_status == EMPLOYMENT_STATUS_RESIGNED {
        return Err(AppError::Biz("该员工已离职，不能提交请假单".into()));
    }
    time_off_repo::find_time_off_type_by_id(txn, req.time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{}", req.time_off_type_id)))?;

    let created = time_off_repo::create_request_in_tx(
        txn,
        hr_time_off_request::ActiveModel {
            employee_id: Set(req.employee_id),
            time_off_type_id: Set(req.time_off_type_id),
            start_at: Set(start_at),
            end_at: Set(end_at),
            duration_minutes: Set(0), // 提交时派生
            reason: Set(req.reason.clone()),
            attachment_id: Set(req.attachment_id),
            status: Set(REQUEST_STATUS_REJECTED), // 提交前视为终态（提交会改写为审批中）
            approval_instance_id: Set(0),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    submit_time_off_request_in_tx(txn, actor_id, created.id).await
}

/// 事务内修改请假单：仅「已驳回 / 已撤销」可改（改完需重新提交）。
pub(crate) async fn update_time_off_request_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateTimeOffRequestReq,
) -> Result<hr_time_off_request::Model, AppError> {
    let request = time_off_repo::find_request_by_id_for_update(txn, req.id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("请假单不存在：{}", req.id)))?;
    ensure_request_owner(txn, actor_id, &request).await?;
    // 状态判定必须在行锁内做：与并发 `submit` 交错时，普通读会拿旧状态通过判断，
    // 等拿到锁后把一张已进入审批的单据内容改掉（区间与已派生时长 / 预占额度不一致）
    if !matches!(
        request.status,
        REQUEST_STATUS_REJECTED | REQUEST_STATUS_CANCELED
    ) {
        return Err(AppError::Biz(
            "审批中或已通过的请假单不能修改，请先撤销".into(),
        ));
    }

    let start_at = parse_request_datetime("请假开始时间", &req.start_at)?;
    let end_at = parse_request_datetime("请假结束时间", &req.end_at)?;
    time_off_repo::find_time_off_type_by_id(txn, req.time_off_type_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("假期类型不存在：{}", req.time_off_type_id)))?;

    Ok(time_off_repo::update_request_in_tx(
        txn,
        hr_time_off_request::ActiveModel {
            id: Set(req.id),
            time_off_type_id: Set(req.time_off_type_id),
            start_at: Set(start_at),
            end_at: Set(end_at),
            reason: Set(req.reason.clone()),
            attachment_id: Set(req.attachment_id),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 事务内撤销请假单：仅「审批中」可撤销；先置「已撤销」再撤销审批实例
/// （审批侧终态分派会释放预占，见 [`on_instance_finished_in_tx`]）。
pub(crate) async fn cancel_time_off_request_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<hr_time_off_request::Model, AppError> {
    // 锁序统一「审批实例 → 业务单据」（与审批侧 approve / reject 一致）：
    // 反序会与并发的审批动作构成 ABBA 环路，被 MySQL 以 1213 杀掉其中一个事务。
    crate::modules::biz::hr::approval::service::lock_latest_instance_by_biz_in_tx(
        txn,
        crate::modules::biz::hr::approval::BIZ_TYPE_TIME_OFF,
        id,
    )
    .await?;

    let request = time_off_repo::find_request_by_id_for_update(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("请假单不存在：{id}")))?;
    if request.status != REQUEST_STATUS_PENDING {
        return Err(AppError::Biz("只有审批中的请假单可以撤销".into()));
    }
    ensure_request_owner(txn, actor_id, &request).await?;

    let updated = time_off_repo::update_request_in_tx(
        txn,
        hr_time_off_request::ActiveModel {
            id: Set(request.id),
            status: Set(REQUEST_STATUS_CANCELED),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    crate::modules::biz::hr::approval::service::cancel_by_biz_in_tx(
        txn,
        actor_id,
        crate::modules::biz::hr::approval::BIZ_TYPE_TIME_OFF,
        request.id,
    )
    .await?;

    Ok(updated)
}

/// 事务内删除请假单（软删）：审批中的单据必须先撤销。
pub(crate) async fn delete_time_off_request_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let request = time_off_repo::find_request_by_id(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("请假单不存在：{id}")))?;
    if request.status == REQUEST_STATUS_PENDING {
        return Err(AppError::Biz("审批中的请假单不能删除，请先撤销".into()));
    }
    ensure_request_owner(txn, actor_id, &request).await?;

    if !time_off_repo::soft_delete_request_in_tx(txn, id, actor_id).await? {
        return Err(AppError::Biz(format!("请假单不存在：{id}")));
    }
    Ok(())
}

/// 审批终态回调（由 `hr/approval` 按 `biz_type = timeOff` 分派，同事务）：
/// 通过 → 预占转实扣；驳回 / 撤销 → 释放预占；随后落单据终态。
///
/// 幂等：实扣只在「审批中」时执行一次；释放由 [`release_locked_in_tx`] 的
/// 「已有释放流水即跳过」兜底，重复回调无害。
pub(crate) async fn on_instance_finished_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    instance_id: u64,
    biz_id: u64,
    instance_status: i8,
) -> Result<(), AppError> {
    use crate::modules::biz::hr::approval::{INSTANCE_STATUS_APPROVED, INSTANCE_STATUS_CANCELED};

    let request = time_off_repo::find_request_by_id_for_update(txn, biz_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("请假单不存在：{biz_id}")))?;
    let on_date = request.start_at.date();

    if instance_status == INSTANCE_STATUS_APPROVED {
        if request.status == REQUEST_STATUS_PENDING {
            consume_locked_in_tx(
                txn,
                actor_id,
                request.employee_id,
                request.time_off_type_id,
                SOURCE_KIND_TIME_OFF,
                instance_id,
                on_date,
            )
            .await?;
            time_off_repo::update_request_in_tx(
                txn,
                hr_time_off_request::ActiveModel {
                    id: Set(request.id),
                    status: Set(REQUEST_STATUS_APPROVED),
                    ..Default::default()
                },
                actor_id,
            )
            .await?;
        }
        return Ok(());
    }

    // 驳回 / 撤销都释放本轮预占（来源 = 本次提交的审批实例）
    release_locked_in_tx(
        txn,
        actor_id,
        request.employee_id,
        request.time_off_type_id,
        SOURCE_KIND_TIME_OFF,
        instance_id,
        on_date,
    )
    .await?;
    // 终态三态分派：撤销（申请人撤回 / 从审批中心撤销）落「已撤销」，
    // 只有驳回才落「已驳回」；业务入口已先置位的情况不改写。
    if request.status == REQUEST_STATUS_PENDING {
        let terminal = if instance_status == INSTANCE_STATUS_CANCELED {
            REQUEST_STATUS_CANCELED
        } else {
            REQUEST_STATUS_REJECTED
        };
        time_off_repo::update_request_in_tx(
            txn,
            hr_time_off_request::ActiveModel {
                id: Set(request.id),
                status: Set(terminal),
                ..Default::default()
            },
            actor_id,
        )
        .await?;
    }
    Ok(())
}

/// 解析请假起止时间（`yyyy-MM-dd HH:mm:ss` 或 `yyyy-MM-dd`）。
fn parse_request_datetime(field: &str, raw: &str) -> Result<DateTime, AppError> {
    crate::utils::datetime::parse_datetime(field, &Some(raw.trim().to_string()), false)?
        .ok_or_else(|| AppError::Biz(format!("{field}不能为空")))
}

/// 请假单分页。
pub async fn page_time_off_requests(
    db: &impl ConnectionTrait,
    req: &TimeOffRequestListReq,
) -> Result<PageData<hr_time_off_request::Model>, AppError> {
    let filter = TimeOffRequestFilter {
        employee_id: req.employee_id,
        time_off_type_id: req.time_off_type_id,
        status: req.status,
        start_at_begin: crate::utils::datetime::parse_datetime(
            "startAtBegin",
            &req.start_at_begin,
            false,
        )?,
        start_at_end: crate::utils::datetime::parse_datetime(
            "startAtEnd",
            &req.start_at_end,
            true,
        )?,
        approval_instance_id: None,
    };
    Ok(
        time_off_repo::find_request_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 我的请假单分页（按当前登录用户的员工档案过滤）。
pub async fn page_my_time_off_requests(
    db: &impl ConnectionTrait,
    user_id: u64,
    req: &MineTimeOffRequestReq,
) -> Result<PageData<hr_time_off_request::Model>, AppError> {
    let employee = require_my_employee(db, user_id).await?;
    let filter = TimeOffRequestFilter {
        employee_id: Some(employee.id),
        status: req.status,
        ..Default::default()
    };
    Ok(
        time_off_repo::find_request_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 请假单详情（软删视为不存在）。
pub async fn get_time_off_request(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_time_off_request::Model, AppError> {
    time_off_repo::find_request_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("请假单不存在：{id}")))
}

// —— 请假单：自持事务入口（api 层调用）——

/// 创建并提交请假单（自持事务）。
pub async fn create_time_off_request(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateTimeOffRequestReq,
) -> Result<hr_time_off_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_time_off_request_in_tx(&txn, actor_id, &req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 修改请假单（自持事务）。
pub async fn update_time_off_request(
    db: &DatabaseConnection,
    actor_id: u64,
    req: UpdateTimeOffRequestReq,
) -> Result<hr_time_off_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_time_off_request_in_tx(&txn, actor_id, &req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 重新提交请假单（自持事务）。
pub async fn submit_time_off_request(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<hr_time_off_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = submit_time_off_request_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 撤销请假单（自持事务）。
pub async fn cancel_time_off_request(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<hr_time_off_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = cancel_time_off_request_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 删除请假单（自持事务，软删）。
pub async fn delete_time_off_request(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_time_off_request_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::hr_employee;
    use crate::modules::biz::hr::time_off::dto::{BatchCreateGrantReq, TimeOffBalanceLogFilter};
    use crate::modules::biz::hr::time_off::{
        GRANT_SOURCE_ISSUE, GRANT_SOURCE_MANUAL, LOG_BIZ_GRANT, LOG_BIZ_TIME_OFF_RELEASE,
        SOURCE_KIND_MANUAL, SOURCE_KIND_TIME_OFF,
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
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
        // 预占即扣批次：lock 已把量从临期批次移走（过期 job 因此看不到在途量），
        // 但**不翻转**批次状态——剩余归零可能只是被预占，驳回还要还回去
        let preoccupied = repo::find_grant_by_id(&txn, expiring)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            preoccupied.remaining_minutes, 0,
            "预占必须直接扣到临期批次的剩余"
        );
        assert_eq!(
            preoccupied.status,
            crate::modules::biz::hr::time_off::GRANT_STATUS_ACTIVE,
            "预占不翻转批次状态（后续可能被释放回滚）"
        );
        service::consume_locked_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
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
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        let err = service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
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
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            240,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
        .await
        .unwrap();
        service::release_locked_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
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
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
        .await
        .unwrap();
        service::consume_locked_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
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
        // 不变式 B（预占即扣批次后重述）：`Σ 未失效批次剩余 + locked == granted + adjust − used − expired`。
        // 在途预占已从批次 remaining 移走，必须由等号左边的 `locked` 项对冲——旧表述
        // `Σ remaining == granted − used` 在本口径下必然破。
        assert_eq!(
            remaining_sum + i64::from(bal.locked_minutes),
            i64::from(bal.granted_minutes) + i64::from(bal.adjust_minutes)
                - i64::from(bal.used_minutes)
                - i64::from(bal.expired_minutes),
            "不变式 B：批次剩余 + 在途预占必须等于 累计授予 + 调整 − 实扣 − 作废"
        );
    }

    /// 回归（P0–P1 真实缺陷）：在途预占跨过批次失效日，过期 job 不得作废在途量。
    ///
    /// 场景：2026 年度 1 天额度、批次 2026-03-31 失效；员工 2026-03-30 提交（预占），
    /// 审批拖到 4-2 才驳回。旧口径下 job 会把这 480 分钟当「批次剩余」作废，
    /// 驳回释放再加回 480 → 可用额度凭空多出一天（甚至因负余额变成负数）。
    #[tokio::test]
    async fn expire_job_does_not_touch_in_flight_preoccupation() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type_with(&txn, 0 /* allow_negative */).await;
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
            Some(date(2026, 3, 31)),
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        // 在批次失效日之前预占（请假单 999）
        service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 3, 30),
        )
        .await
        .unwrap();

        let expired = service::expire_grants_in_tx(&txn, date(2026, 4, 2))
            .await
            .unwrap();
        assert_eq!(expired, 0, "只剩在途预占的批次不得被作废");

        // 审批驳回：释放预占（归还到原批次，因为按单据发生日它尚未失效）
        service::release_locked_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 3, 30),
        )
        .await
        .unwrap();

        let bal = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bal.locked_minutes, 0, "释放后预占归零");
        assert_eq!(bal.expired_minutes, 0, "在途量不得被计入作废");
        assert_eq!(bal.used_minutes, 0, "驳回不得计入实扣");
        assert_eq!(
            service::available_minutes(&txn, emp, ty, "2026", date(2026, 4, 2))
                .await
                .unwrap(),
            480,
            "驳回后可用额度应恢复为 1 天（既不凭空多出、也不归零）"
        );
        // 释放后额度回到原批次：该批次仍会在下一次过期任务里被正常作废（走常规路径）
        let grants = repo::find_grant_page(
            &txn,
            &TimeOffGrantFilter {
                employee_id: Some(emp),
                time_off_type_id: Some(ty),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            grants
                .items
                .iter()
                .map(|g| g.remaining_minutes)
                .sum::<i32>(),
            480,
            "释放必须把额度还回批次，而不是只改账户"
        );
    }

    /// 驳回释放时原批次已失效 / 已撤销：归还到当前账期的「归还批次」（幂等键 = 请假单）。
    ///
    /// 用 HR 撤销批次（`cancel_grant_in_tx`）制造「原批次不可归还」的场景：在途预占不受撤销影响，
    /// 驳回归还时原批次已是 `CANCELED`，只能落到当前账期的归还批次；账户 `granted` 不再增加
    /// （额度不是新授予），不变式 B 仍成立。
    #[tokio::test]
    async fn release_restores_to_current_period_batch_when_original_grant_is_revoked() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type_with(&txn, 0).await;
        let batch = service::grant_time_off_in_tx(
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
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            480,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
        .await
        .unwrap();
        // HR 撤销该批次：剩余为 0（全被预占），故 granted 不动、状态置已撤销
        super::cancel_grant_in_tx(&txn, ACTOR_ID, batch)
            .await
            .unwrap();

        service::release_locked_in_tx(
            &txn,
            ACTOR_ID,
            emp,
            ty,
            SOURCE_KIND_TIME_OFF,
            999,
            date(2026, 2, 1),
        )
        .await
        .unwrap();

        let restore = repo::find_grant_by_source_key(&txn, emp, ty, SOURCE_KIND_TIME_OFF, 999)
            .await
            .unwrap()
            .expect("应生成当前账期的归还批次");
        assert_eq!(restore.remaining_minutes, 480, "归还批次应承载归还量");
        assert_eq!(restore.minutes, 0, "归还不是新授予；minutes 不含新增授予量");

        let bal = repo::find_balance_by_account_for_update(&txn, emp, ty, "2026")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bal.granted_minutes, 480, "归还不得增加累计授予");
        assert_eq!(bal.locked_minutes, 0, "归还后预占归零");
        assert_eq!(
            service::available_minutes(&txn, emp, ty, "2026", date(2026, 2, 1))
                .await
                .unwrap(),
            480,
            "归还的量必须重新可用"
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

#[cfg(test)]
mod request_tests {
    use super::*;
    use crate::entity::{hr_employee, hr_time_off_type, hr_work_calendar, sys_user_dept};
    use crate::modules::biz::hr::approval;
    use crate::modules::biz::hr::time_off::dto::{CreateTimeOffRequestReq, TimeOffGrantFilter};
    use crate::modules::biz::hr::time_off::{
        BALANCE_MODE_DEDUCT, BALANCE_MODE_RECORD_ONLY, GRANT_SOURCE_ISSUE, GRANT_STATUS_CANCELED,
        LOG_BIZ_TIME_OFF_LOCK, LOG_BIZ_TIME_OFF_RELEASE, REQUEST_STATUS_APPROVED,
        REQUEST_STATUS_CANCELED, REQUEST_STATUS_PENDING, REQUEST_STATUS_REJECTED,
        SOURCE_KIND_MANUAL, TIME_OFF_TYPE_ENABLED, repo, service,
    };
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 测试统一用种子 admin（id = 1）作操作人。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);
    static DAY_SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一「用户 ID」段位 900_5xx（审批域用 900_4xx，避免并行撞键）。
    fn unique_user_id() -> u64 {
        900_500_000
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed) % 1_000
    }

    /// 唯一自然日：2030-01-01 起逐日递增（`hr_work_calendar.calendar_date` 是全局唯一键，
    /// 固定日期会与其它用例互撞）。取值在 2030 年之后，不会碰到真实业务数据。
    fn unique_work_date() -> chrono::NaiveDate {
        let offset = DAY_SEQ.fetch_add(1, Ordering::Relaxed) % 3_000;
        chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap() + chrono::Duration::days(offset as i64)
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    async fn test_txn() -> DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 该日为工作日（无排班 → 走日历的「标准工作制」分支：窗口 = 整个自然日、
    /// 当日封顶 `standard_minutes = 480`）。
    async fn seed_workday(txn: &DatabaseTransaction, day: chrono::NaiveDate) {
        hr_work_calendar::ActiveModel {
            calendar_date: Set(day),
            is_workday: Set(1),
            holiday_type: Set(0),
            standard_minutes: Set(480),
            created_by: Set(ACTOR_ID),
            updated_by: Set(ACTOR_ID),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap();
    }

    /// 假期类型（`balance_mode` 决定是否走额度账本）。
    async fn seed_type(txn: &DatabaseTransaction, balance_mode: i8) -> u64 {
        hr_time_off_type::ActiveModel {
            type_code: Set(unique("req_type")),
            type_name: Set("测试假别".to_string()),
            unit: Set(1),
            balance_mode: Set(balance_mode),
            min_unit_minutes: Set(60),
            require_attachment: Set(0),
            allow_negative: Set(0),
            status: Set(TIME_OFF_TYPE_ENABLED),
            created_by: Set(ACTOR_ID),
            updated_by: Set(ACTOR_ID),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap()
        .id
    }

    async fn seed_employee(
        txn: &DatabaseTransaction,
        user_id: u64,
        manager_employee_id: u64,
    ) -> u64 {
        hr_employee::ActiveModel {
            user_id: Set(user_id),
            manager_employee_id: Set(manager_employee_id),
            employment_status: Set(1),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap()
        .id
    }

    /// 直插一个启用账号并返回其自动生成的 ID（审批人解析要求账号存在且启用）。
    ///
    /// 用自动 ID 而不是显式大 ID：显式 ID 会把 `sys_user` 的 AUTO_INCREMENT 顶到高位
    /// （计数器不随事务回滚），后续真实用户就会拿到 900M 段的 ID。
    async fn seed_enabled_user(txn: &DatabaseTransaction) -> u64 {
        crate::entity::sys_user::ActiveModel {
            username: Set(unique("req_user")),
            password: Set("x".to_string()),
            emp_no: Set(String::new()),
            nickname: Set("请假用例用户".to_string()),
            status: Set(1),
            created_by: Set(ACTOR_ID),
            updated_by: Set(ACTOR_ID),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap()
        .id
    }

    /// 申请人 + 直属上级（同一个人同时是主部门负责人），返回 `(员工, 申请人 user_id, 上级 user_id)`。
    ///
    /// 种子形状的 `timeOff` 流是「直属上级（可跳过）→ 部门负责人」，两级因此解析到同一人。
    /// 两人的账号必须**启用**：审批人解析会校验账号可用（停用 / 无账号视为解析不到审批人）。
    async fn seed_applicant(txn: &DatabaseTransaction) -> (u64, u64, u64) {
        let manager_user = seed_enabled_user(txn).await;
        let applicant_user = seed_enabled_user(txn).await;
        let dept_id = unique_user_id();

        let manager_employee = seed_employee(txn, manager_user, 0).await;
        let applicant_employee = seed_employee(txn, applicant_user, manager_employee).await;

        for (user_id, is_primary, is_leader) in [(applicant_user, 1, 0), (manager_user, 0, 1)] {
            sys_user_dept::ActiveModel {
                user_id: Set(user_id),
                dept_id: Set(dept_id),
                is_primary: Set(is_primary),
                is_leader: Set(is_leader),
            }
            .insert(txn)
            .await
            .unwrap();
        }
        (applicant_employee, applicant_user, manager_user)
    }

    /// 确保 `timeOff` 审批流存在（种子已建则原样复用，**不改动共享模板的节点**）。
    async fn ensure_time_off_flow(txn: &DatabaseTransaction) {
        if approval::repo::find_enabled_flow_by_biz_type(txn, approval::BIZ_TYPE_TIME_OFF)
            .await
            .unwrap()
            .is_some()
        {
            return;
        }
        let flow = approval::repo::create_flow_in_tx(
            txn,
            crate::entity::hr_approval_flow::ActiveModel {
                biz_type: Set(approval::BIZ_TYPE_TIME_OFF.to_string()),
                name: Set("请假审批流".to_string()),
                status: Set(approval::FLOW_STATUS_ENABLED),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();
        for (seq, node_type, skip_if_empty) in [
            (1, approval::NODE_TYPE_MANAGER, 1),
            (2, approval::NODE_TYPE_DEPT_LEADER, 0),
        ] {
            approval::repo::create_node_in_tx(
                txn,
                crate::entity::hr_approval_flow_node::ActiveModel {
                    flow_id: Set(flow.id),
                    seq: Set(seq),
                    node_name: Set(format!("节点 {seq}")),
                    node_type: Set(node_type),
                    approver_ref_id: Set(0),
                    skip_if_empty: Set(skip_if_empty),
                    ..Default::default()
                },
                ACTOR_ID,
            )
            .await
            .unwrap();
        }
    }

    /// 建单请求（`duration_minutes` 不在请求体里 —— 由后端按排班 × 日历派生）。
    fn request_req(
        employee_id: u64,
        type_id: u64,
        day: chrono::NaiveDate,
        from: u32,
        to: u32,
    ) -> CreateTimeOffRequestReq {
        CreateTimeOffRequestReq {
            employee_id,
            time_off_type_id: type_id,
            start_at: day
                .and_hms_opt(from, 0, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            end_at: day
                .and_hms_opt(to, 0, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            reason: "测试请假".to_string(),
            attachment_id: 0,
            remark: String::new(),
        }
    }

    /// 走完「种子形状」的两级审批（直属上级 → 部门负责人，同一人）。
    async fn approve_two_nodes(txn: &DatabaseTransaction, instance_id: u64, approver: u64) {
        for step in 1..=2 {
            approval::service::approve_in_tx(txn, approver, instance_id, "同意")
                .await
                .unwrap_or_else(|e| panic!("第 {step} 级审批失败：{e}"));
        }
    }

    /// 提交请假单：后端派生时长 240、状态置审批中、预占额度 240、起审批实例。
    #[tokio::test]
    async fn submit_derives_duration_locks_quota_and_starts_approval() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();

        assert_eq!(
            request.duration_minutes, 240,
            "09:00–13:00 落在无排班工作日内应派生 240 分钟"
        );
        assert_eq!(request.status, REQUEST_STATUS_PENDING);
        assert!(request.approval_instance_id > 0, "提交必须起审批实例");

        let (instance, records) =
            approval::service::get_instance(&txn, request.approval_instance_id)
                .await
                .unwrap();
        assert_eq!(instance.status, approval::INSTANCE_STATUS_PENDING);
        assert_eq!(instance.biz_id, request.id, "实例应指向该请假单");
        assert_eq!(
            instance.current_approver_id, manager_user,
            "第一级应解析到申请人的直属上级"
        );
        assert_eq!(records.len(), 2, "两级节点一次展开");

        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .expect("应建额度账户");
        assert_eq!(balance.granted_minutes, 480);
        assert_eq!(balance.locked_minutes, 240, "提交即预占 240 分钟");
        assert_eq!(balance.used_minutes, 0, "审批中不得实扣");

        let grants = repo::find_grant_page(
            &txn,
            &TimeOffGrantFilter {
                employee_id: Some(employee_id),
                time_off_type_id: Some(type_id),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            grants
                .items
                .iter()
                .map(|g| g.remaining_minutes)
                .sum::<i32>(),
            240,
            "预占即扣批次：批次剩余必须同步减少"
        );
    }

    /// 通过 → 预占转实扣；驳回 → 释放预占；两条路径都要落对账户字段与流水。
    #[tokio::test]
    async fn approve_consumes_quota_and_reject_releases_it() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let (day_ok, day_no) = (unique_work_date(), unique_work_date());
        seed_workday(&txn, day_ok).await;
        seed_workday(&txn, day_no).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            960,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day_ok,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let approved_request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day_ok, 9, 13),
        )
        .await
        .unwrap();
        approve_two_nodes(&txn, approved_request.approval_instance_id, manager_user).await;

        let rejected_request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day_no, 9, 13),
        )
        .await
        .unwrap();
        approval::service::reject_in_tx(
            &txn,
            manager_user,
            rejected_request.approval_instance_id,
            "不批",
        )
        .await
        .unwrap();

        let reloaded_ok = service::get_time_off_request(&txn, approved_request.id)
            .await
            .unwrap();
        assert_eq!(
            reloaded_ok.status, REQUEST_STATUS_APPROVED,
            "审批通过应落已通过"
        );
        let reloaded_no = service::get_time_off_request(&txn, rejected_request.id)
            .await
            .unwrap();
        assert_eq!(
            reloaded_no.status, REQUEST_STATUS_REJECTED,
            "驳回应落已驳回"
        );

        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.used_minutes, 240, "只有通过的那张单计入实扣");
        assert_eq!(balance.locked_minutes, 0, "两张单都应离开预占态");
        assert_eq!(
            service::available_minutes(&txn, employee_id, type_id, "2030", day_ok)
                .await
                .unwrap(),
            720,
            "可用 = 960 − 240（驳回的那 240 已释放）"
        );

        let release_logs = repo::find_logs_by_source(
            &txn,
            SOURCE_KIND_TIME_OFF,
            rejected_request.approval_instance_id,
            LOG_BIZ_TIME_OFF_RELEASE,
        )
        .await
        .unwrap();
        assert!(!release_logs.is_empty(), "驳回必须写释放流水");
        let lock_logs = repo::find_logs_by_source(
            &txn,
            SOURCE_KIND_TIME_OFF,
            approved_request.approval_instance_id,
            LOG_BIZ_TIME_OFF_LOCK,
        )
        .await
        .unwrap();
        assert!(!lock_logs.is_empty(), "通过的单据应有预占流水");
        assert!(
            lock_logs.iter().all(|log| log.grant_id != 0),
            "预占流水必须指向具体批次（预占即扣批次）"
        );
    }

    /// 改单请求（内容与 `request_req` 同区间，仅用于「驳回后修改再提交」）。
    fn update_req_of(
        id: u64,
        type_id: u64,
        day: chrono::NaiveDate,
    ) -> crate::modules::biz::hr::time_off::dto::UpdateTimeOffRequestReq {
        crate::modules::biz::hr::time_off::dto::UpdateTimeOffRequestReq {
            id,
            time_off_type_id: type_id,
            start_at: day
                .and_hms_opt(9, 0, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            end_at: day
                .and_hms_opt(13, 0, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            reason: "改后再提".to_string(),
            attachment_id: 0,
            remark: String::new(),
        }
    }

    /// 驳回后「修改 + 重新提交」，返回重新提交后的单据（第二轮审批实例）。
    async fn resubmit_after_rejection(
        txn: &DatabaseTransaction,
        applicant_user: u64,
        request_id: u64,
        type_id: u64,
        day: chrono::NaiveDate,
    ) -> hr_time_off_request::Model {
        service::update_time_off_request_in_tx(
            txn,
            applicant_user,
            &update_req_of(request_id, type_id, day),
        )
        .await
        .unwrap();
        service::submit_time_off_request_in_tx(txn, applicant_user, request_id)
            .await
            .unwrap()
    }

    /// 驳回 → 修改 → 重新提交 → 再驳回：两轮预占必须各自独立结算。
    ///
    /// 预占 / 释放流水按「一次提交周期」（审批实例）记账，第二轮释放不能因
    /// 「该单据已有释放流水」被整体跳过 —— 否则 `locked` 永久滞留在账户上，可用额度静默少一块。
    #[tokio::test]
    async fn resubmit_cycle_releases_quota_of_each_round() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();
        approval::service::reject_in_tx(&txn, manager_user, request.approval_instance_id, "不批")
            .await
            .unwrap();

        let resubmitted =
            resubmit_after_rejection(&txn, applicant_user, request.id, type_id, day).await;
        assert_ne!(
            resubmitted.approval_instance_id, request.approval_instance_id,
            "重新提交必须另起审批实例（终态实例不可推进）"
        );
        approval::service::reject_in_tx(
            &txn,
            manager_user,
            resubmitted.approval_instance_id,
            "还是不批",
        )
        .await
        .unwrap();

        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.locked_minutes, 0, "两轮驳回后预占必须全部释放");
        assert_eq!(balance.used_minutes, 0, "驳回不得实扣");
        assert_eq!(
            service::available_minutes(&txn, employee_id, type_id, "2030", day)
                .await
                .unwrap(),
            480,
            "可用额度必须回到初始值"
        );
    }

    /// 驳回 → 修改 → 重新提交 → 通过：第二轮审批必须能通过（不得因两轮预占求和而报
    /// 「预占量不足」把单据卡死），且只实扣一次。
    #[tokio::test]
    async fn resubmit_cycle_can_be_approved_once() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();
        approval::service::reject_in_tx(&txn, manager_user, request.approval_instance_id, "不批")
            .await
            .unwrap();
        let resubmitted =
            resubmit_after_rejection(&txn, applicant_user, request.id, type_id, day).await;
        approve_two_nodes(&txn, resubmitted.approval_instance_id, manager_user).await;

        let reloaded = service::get_time_off_request(&txn, request.id)
            .await
            .unwrap();
        assert_eq!(
            reloaded.status, REQUEST_STATUS_APPROVED,
            "第二轮审批通过后单据应为已通过"
        );
        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.used_minutes, 240, "只实扣一次");
        assert_eq!(balance.locked_minutes, 0, "通过后不得残留预占");
        assert_eq!(
            service::available_minutes(&txn, employee_id, type_id, "2030", day)
                .await
                .unwrap(),
            240,
            "可用 = 480 − 240"
        );
    }

    /// 已通过的请假单不允许再次提交（`submit` 仅限「已驳回 / 已撤销」）：
    /// 放行会二次预占额度并开出第二个审批实例。
    #[tokio::test]
    async fn approved_request_cannot_be_submitted_again() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();
        approve_two_nodes(&txn, request.approval_instance_id, manager_user).await;

        let err = service::submit_time_off_request_in_tx(&txn, applicant_user, request.id)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("已驳回") || err.to_string().contains("已撤销"),
            "已通过单据重提必须被拒，实际：{err}"
        );

        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.locked_minutes, 0, "重提不得二次预占");
        assert_eq!(balance.used_minutes, 240, "实扣仍只算一次");
    }

    /// 申请人从审批中心撤销（`instance/cancel`）时，业务单据必须落「已撤销」而不是「已驳回」。
    #[tokio::test]
    async fn cancel_from_approval_center_marks_request_canceled() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, _manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();
        approval::service::cancel_instance_in_tx(
            &txn,
            applicant_user,
            request.approval_instance_id,
        )
        .await
        .unwrap();

        let reloaded = service::get_time_off_request(&txn, request.id)
            .await
            .unwrap();
        assert_eq!(
            reloaded.status, REQUEST_STATUS_CANCELED,
            "从审批中心撤销应落「已撤销」（终态三态不得被压成「已驳回」）"
        );
        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.locked_minutes, 0, "撤销必须释放预占");
    }

    /// 他人不得提交 / 修改别人的请假单（与 `cancel` / `delete` 的属主口径一致）。
    #[tokio::test]
    async fn submit_and_update_reject_requests_of_other_employees() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let other_user = unique_user_id();
        seed_employee(&txn, other_user, 0).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();
        approval::service::reject_in_tx(&txn, manager_user, request.approval_instance_id, "不批")
            .await
            .unwrap();

        let err = service::update_time_off_request_in_tx(
            &txn,
            other_user,
            &update_req_of(request.id, type_id, day),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("本人"),
            "他人不得修改别人的请假单，实际：{err}"
        );
        let err = service::submit_time_off_request_in_tx(&txn, other_user, request.id)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("本人"),
            "他人不得提交别人的请假单，实际：{err}"
        );
    }

    /// 并发提交之后 `update` 必须看到最新状态（状态判定要在行锁内做）。
    ///
    /// 另一连接把单据推进到「审批中」并提交后，本连接的 `update` 必须拒绝，
    /// 而不是把一张已进入审批的单据内容改掉（区间会与已派生时长 / 预占额度不一致）。
    ///
    /// 跨连接可见性必须提交，故用真库 + 显式清理（不能用 `test_txn()` 的回滚隔离）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn update_rejects_when_concurrent_submit_moved_it_to_pending() {
        use sea_orm::{EntityTrait, TransactionTrait};

        let db = test_db().await;
        let applicant_user = unique_user_id();
        let setup = db.begin().await.unwrap();
        let employee_id = seed_employee(&setup, applicant_user, 0).await;
        let type_id = seed_type(&setup, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        let request_id = repo::create_request_in_tx(
            &setup,
            hr_time_off_request::ActiveModel {
                employee_id: Set(employee_id),
                time_off_type_id: Set(type_id),
                start_at: Set(day.and_hms_opt(9, 0, 0).unwrap()),
                end_at: Set(day.and_hms_opt(13, 0, 0).unwrap()),
                duration_minutes: Set(240),
                reason: Set("并发用例".to_string()),
                status: Set(REQUEST_STATUS_REJECTED),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap()
        .id;
        setup.commit().await.unwrap();

        // ① 本连接先建立快照：此刻看到的是「已驳回」
        let stale = db.begin().await.unwrap();
        let seen = repo::find_request_by_id(&stale, request_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            seen.status, REQUEST_STATUS_REJECTED,
            "前置：单据初始为已驳回"
        );

        // ② 另一连接把单据推进到「审批中」并提交（模拟并发的 submit）
        let concurrent = db.begin().await.unwrap();
        repo::update_request_in_tx(
            &concurrent,
            hr_time_off_request::ActiveModel {
                id: Set(request_id),
                status: Set(REQUEST_STATUS_PENDING),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();
        concurrent.commit().await.unwrap();

        // ③ 本连接再 update：必须按最新状态拒绝
        let err = service::update_time_off_request_in_tx(
            &stale,
            applicant_user,
            &update_req_of(request_id, type_id, day),
        )
        .await;

        // **先清理再断言**：夹具行是提交过的（跨连接可见性必须提交），
        // 断言失败时若还没清理就会把已提交的行留在库里（同 `probe_*` 口径）
        drop(stale);
        crate::entity::hr_time_off_request::Entity::delete_by_id(request_id)
            .exec(&db)
            .await
            .unwrap();
        crate::entity::hr_time_off_type::Entity::delete_by_id(type_id)
            .exec(&db)
            .await
            .unwrap();
        crate::entity::hr_employee::Entity::delete_by_id(employee_id)
            .exec(&db)
            .await
            .unwrap();

        let err = err.unwrap_err();
        assert!(
            err.to_string().contains("审批中") || err.to_string().contains("已通过"),
            "单据已被并发推进到审批中，update 必须拒绝，实际：{err}"
        );
    }

    /// 同一员工区间重叠的请假单必须被拒（跨假别的「读 → 判断 → 写」，靠员工行锁串行化）。
    #[tokio::test]
    async fn overlapping_request_is_rejected() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, _manager) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            960,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let _first = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();

        let err = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 11, 15),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("重叠"),
            "重叠区间必须被拒，实际：{err}"
        );
    }

    /// 记录型假别（`balance_mode = 0`）：不建账户、不预占、不写流水，但单据与审批照常。
    #[tokio::test]
    async fn record_only_type_skips_ledger_entirely() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, manager_user) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_RECORD_ONLY).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();
        assert_eq!(request.duration_minutes, 240, "记录型假别照样派生时长");
        assert_eq!(request.status, REQUEST_STATUS_PENDING);
        assert!(
            repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
                .await
                .unwrap()
                .is_none(),
            "记录型假别不得建额度账户"
        );

        approve_two_nodes(&txn, request.approval_instance_id, manager_user).await;
        let reloaded = service::get_time_off_request(&txn, request.id)
            .await
            .unwrap();
        assert_eq!(reloaded.status, REQUEST_STATUS_APPROVED);
        let logs = repo::find_logs_by_source(
            &txn,
            SOURCE_KIND_TIME_OFF,
            request.id,
            LOG_BIZ_TIME_OFF_LOCK,
        )
        .await
        .unwrap();
        assert!(logs.is_empty(), "记录型假别不得写额度流水");
    }

    /// 撤销：申请人本人可撤销，撤销后释放预占、状态置已撤销。
    #[tokio::test]
    async fn cancel_releases_quota_and_marks_canceled() {
        let txn = test_txn().await;
        ensure_time_off_flow(&txn).await;
        let (employee_id, applicant_user, _manager) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        seed_workday(&txn, day).await;
        service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();

        let request = service::create_time_off_request_in_tx(
            &txn,
            applicant_user,
            &request_req(employee_id, type_id, day, 9, 13),
        )
        .await
        .unwrap();

        let err = service::cancel_time_off_request_in_tx(&txn, ACTOR_ID, request.id)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("只能操作本人"),
            "非申请人不得撤销，实际：{err}"
        );

        let canceled = service::cancel_time_off_request_in_tx(&txn, applicant_user, request.id)
            .await
            .unwrap();
        assert_eq!(canceled.status, REQUEST_STATUS_CANCELED);

        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(balance.locked_minutes, 0, "撤销后预占归零");
        assert_eq!(balance.used_minutes, 0, "撤销不得计入实扣");
        assert_eq!(
            service::available_minutes(&txn, employee_id, type_id, "2030", day)
                .await
                .unwrap(),
            480,
            "撤销后可用额度恢复"
        );
        let (instance, _) = approval::service::get_instance(&txn, request.approval_instance_id)
            .await
            .unwrap();
        assert_eq!(
            instance.status,
            approval::INSTANCE_STATUS_CANCELED,
            "撤销请假单必须同时撤销审批实例"
        );
    }

    /// 已通过的批次不得被撤销（`cancel_grant` 的状态护栏）——顺带钉住「预占即扣批次」下
    /// 撤销的剩余口径：批次剩余为 0 时撤销不动账户 `granted`。
    #[tokio::test]
    async fn cancel_grant_keeps_ledger_consistent_while_preoccupied() {
        let txn = test_txn().await;
        let (employee_id, _applicant_user, _manager) = seed_applicant(&txn).await;
        let type_id = seed_type(&txn, BALANCE_MODE_DEDUCT).await;
        let day = unique_work_date();
        let batch = service::grant_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            GRANT_SOURCE_ISSUE,
            "statutory",
            "2030",
            day,
            None,
            SOURCE_KIND_MANUAL,
            0,
        )
        .await
        .unwrap();
        service::lock_time_off_in_tx(
            &txn,
            ACTOR_ID,
            employee_id,
            type_id,
            480,
            SOURCE_KIND_TIME_OFF,
            900_500_777,
            day,
        )
        .await
        .unwrap();

        super::cancel_grant_in_tx(&txn, ACTOR_ID, batch)
            .await
            .unwrap();

        let balance = repo::find_balance_by_account_for_update(&txn, employee_id, type_id, "2030")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            balance.granted_minutes, 480,
            "批次剩余已是 0（全被预占），撤销不得回冲累计授予"
        );
        assert_eq!(balance.locked_minutes, 480, "在途预占不受 HR 撤销影响");
        let grant = repo::find_grant_by_id(&txn, batch).await.unwrap().unwrap();
        assert_eq!(grant.status, GRANT_STATUS_CANCELED, "批次状态应为已撤销");
    }
}
