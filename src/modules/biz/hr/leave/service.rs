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
//! # 账本口径（`hr_leave_balance_log.delta_minutes`）
//!
//! `delta_minutes` 恒等于该动作对**可用余额**的影响（= `after_minutes − before_minutes`），
//! 与 `hr_leave_balance_log` 的 `before_minutes` / `after_minutes` 列注释同构：
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
//! # 账户账期（`hr_leave_balance.period`）
//!
//! 账户账期 = **交易发生日 / 发放生效日的自然年**：
//! - `grant_leave_in_tx` 取 `effective_at` 的自然年——`hr_leave_grant.period` 只是**归属标记**
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

use std::collections::HashMap;

use sea_orm::DatabaseTransaction;
use sea_orm::entity::prelude::*;

use crate::entity::{hr_leave_balance, hr_leave_balance_log, hr_leave_grant, hr_leave_type};
use crate::modules::biz::hr::leave::dto::{
    BatchCreateGrantReq, BatchCreateGrantResp, CreateLeaveTypeReq, LeaveBalanceListReq,
    LeaveBalanceLogListReq, LeaveGrantListReq, LeaveTypeListReq, UpdateLeaveTypeReq,
};
use crate::utils::PageData;
use crate::utils::error::AppError;

// —— 额度原语（pub(crate)：P2 请假 / P4 加班复用；调用方必须自持事务）——

/// 发放额度：幂等键（员工 × 假别 × 依据 × 周期）命中即复用，否则建批次 + 记账户 + 写流水。
///
/// 返回批次 ID。调用方（[`batch_create_grants_in_tx`] / P2 请假 / P4 加班）负责事务边界。
// 实现提示：① 幂等探测 `repo::find_grant_by_idempotent_key(txn, employee_id, leave_type_id, reason, period)`，
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
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
// 成对原语的入参集合固定；拆参数结构体会让跨域调用方多一层构造，收益不足
#[allow(clippy::too_many_arguments)]
pub(crate) async fn grant_leave_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    employee_id: u64,
    leave_type_id: u64,
    minutes: i32,
    source: i8,
    reason: &str,
    period: &str,
    effective_at: Date,
    expire_at: Option<Date>,
) -> Result<u64, AppError> {
    let _ = (
        txn,
        actor_id,
        employee_id,
        leave_type_id,
        minutes,
        source,
        reason,
        period,
        effective_at,
        expire_at,
    );
    Err(AppError::Biz("未实现：grant_leave_in_tx".into()))
}

/// 预占额度（审批中）：账户 `locked_minutes += minutes`，可用不足且假别 `allow_negative = 0` 时拒绝。
///
/// 预占只动 `locked`，**不**动 `used`——实扣在审批通过时由 [`consume_locked_in_tx`] 完成。
// 实现提示：① `repo::find_balance_by_account_for_update(txn, employee_id, leave_type_id, period)` 加锁读，
// 账期取 `on_date` 的自然年；`None` → `AppError::Biz("额度账户不存在，请先发放额度")`；
// ② 账户加锁读后即可用，读 `hr_leave_type`（`repo::find_leave_type_by_id`）取 `allow_negative`；
// ③ `available = granted + adjust − used − locked − expired`；`available < minutes` 且
// `allow_negative == 0` → `AppError::Biz(format!("额度不足：可用 {available} 分钟，本次需要 {minutes} 分钟"))`
// （`allow_negative == 1` 放行，允许负余额）；④ 账户 `locked_minutes += minutes` →
// `repo::update_balance_in_tx`；⑤ 写流水：`biz_type = LOG_BIZ_LEAVE_LOCK`、`delta = −minutes`
// （预占把可用锁住，见文件头账本口径）、`grant_id = 0`（账户级动作）、`source_kind = 0` /
// `source_id = 0`（本签名不带来源单据，P2 接入请假单后由调用方补）。
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn lock_leave_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    leave_type_id: u64,
    minutes: i32,
    on_date: Date,
) -> Result<(), AppError> {
    let _ = (txn, employee_id, leave_type_id, minutes, on_date);
    Err(AppError::Biz("未实现：lock_leave_in_tx".into()))
}

/// 实扣（审批通过）：FEFO 扣批次剩余，账户 `locked -= minutes`、`used += minutes`，可用不变。
///
/// 批次归属：每个被扣的批次写一条流水（`grant_id` 指向它），供「这笔假扣的是哪个批次」追溯。
// 实现提示：① `repo::find_active_grants_for_update(txn, employee_id, leave_type_id, on_date)` FEFO 加锁读；
// ② 账户加锁读（账期取 `on_date` 的自然年）——`locked_minutes -= minutes`、`used_minutes += minutes`，
// 变动前后**可用值相同**；③ 循环扣批次：`take = min(剩余待扣, batch.remaining_minutes)` →
// `repo::consume_grant_in_tx(txn, batch.id, take, actor_id)`（返回 `false` 说明并发下批次已被扣空，
// 重新取批次或直接报错）；扣不满 `minutes` → `AppError::Biz("额度批次不足，请检查账本一致性")`；
// ④ 每个被扣批次：`remaining_minutes == 0` 时
// `repo::set_grant_status_in_tx(txn, batch.id, GRANT_STATUS_EXHAUSTED, actor_id)`，
// 并写一条流水：`biz_type = LOG_BIZ_LEAVE_CONSUME`、`delta = 0`（locked→used，可用不变，
// 见文件头账本口径）、`grant_id = batch.id`、`source_kind` / `source_id` 原样入来源列；
// ⑤ `grant_id` 之外的 `operator_id` 传 0（系统动作），P2 接入请假单后可改为审批人。
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn consume_locked_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    leave_type_id: u64,
    minutes: i32,
    source_kind: i8,
    source_id: u64,
    on_date: Date,
) -> Result<(), AppError> {
    let _ = (
        txn,
        employee_id,
        leave_type_id,
        minutes,
        source_kind,
        source_id,
        on_date,
    );
    Err(AppError::Biz("未实现：consume_locked_in_tx".into()))
}

/// 释放预占（审批驳回 / 撤销）：账户 `locked -= minutes`，可用恢复，流水正向记恢复量。
// 实现提示：① 账户加锁读 `repo::find_balance_by_account_for_update(txn, employee_id, leave_type_id, period)`，
// 账期取 `on_date` 的自然年（`format!("{}", on_date.year())`）；`None` →
// `AppError::Biz("额度账户不存在，请先发放额度")`；② 账户 `locked_minutes -= minutes` →
// `repo::update_balance_in_tx`；③ 写流水：`biz_type = LOG_BIZ_LEAVE_RELEASE`、`delta = +minutes`
// （可用恢复，见文件头账本口径）、`grant_id = 0`（账户级）、`source_kind` / `source_id` 原样入来源列。
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn release_locked_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
    leave_type_id: u64,
    minutes: i32,
    source_kind: i8,
    source_id: u64,
    on_date: Date,
) -> Result<(), AppError> {
    let _ = (
        txn,
        employee_id,
        leave_type_id,
        minutes,
        source_kind,
        source_id,
        on_date,
    );
    Err(AppError::Biz("未实现：release_locked_in_tx".into()))
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
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn expire_grants_in_tx(
    txn: &DatabaseTransaction,
    today: Date,
) -> Result<u64, AppError> {
    let _ = (txn, today);
    Err(AppError::Biz("未实现：expire_grants_in_tx".into()))
}

/// 可用额度（分钟）：账户口径 `granted + adjust − used − locked − expired`。
///
/// `period` 为账期（自然年字符串，调用方按「单据发生日」解析）。返回 `i64`：账户各列是 `i32`，
/// 相减可能越界，聚合口径一律升位到 `i64`。
// 实现提示：① 本签名收 `&DatabaseTransaction`，与加锁读原语同型，直接复用
// `repo::find_balance_by_account_for_update(txn, employee_id, leave_type_id, period)`（无需新 SQL）；
// ② 账户 `None` 视为 0（未发放即无可用量）；③ `on_date` 当前不参与计算（签名保留，供后续
// 「按有效期折算可用」口径），实现时 `let _ = on_date;` 消音。
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn available_minutes(
    txn: &DatabaseTransaction,
    employee_id: u64,
    leave_type_id: u64,
    period: &str,
    on_date: Date,
) -> Result<i64, AppError> {
    let _ = (txn, employee_id, leave_type_id, period, on_date);
    Err(AppError::Biz("未实现：available_minutes".into()))
}

// —— 批量发放（service 编排：范围解析 + 逐人发放；测试要在 test_txn 里跑，故提供 _in_tx 变体）——

/// 批量发放额度（事务内实现）：范围解析 → 逐人调用 [`grant_leave_in_tx`]。
///
/// 幂等：命中「员工 × 假别 × 依据 × 周期」的人计入 `skipped` 并记入 `skipped_employee_ids`
/// （**保持入参顺序**），供前端提示哪些人本周期已发放。
// 实现提示：① 范围解析（三选一，互斥性由 `validate.rs` 保证）——`req.all = true` → 全部在职员工
// （`hr_employee.employment_status != 3`，建议由 employee 域 repo 提供批量原语）；
// `req.dept_id = Some(id)` → 该部门挂载员工（`sys_user_dept`，建议由 dept 域 repo 提供）；
// 否则直接用 `req.employee_ids`（去重后按入参顺序处理）；② 解析 `req.effective_at` /
// `req.expire_at`（`chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")`，失败 →
// `AppError::Biz(format!("日期格式不正确：{s}"))`）；③ 逐人先
// `repo::find_grant_by_idempotent_key(txn, emp, req.leave_type_id, &req.reason, &req.period)` 探测：
// 已存在 → `skipped += 1` + 记入 `skipped_employee_ids`；不存在 → `grant_leave_in_tx(...)` 后
// `created += 1`；④ 返回 `BatchCreateGrantResp`。
// 骨架期：仅测试调用，实现函数体后删除本属性
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn batch_create_grants_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &BatchCreateGrantReq,
) -> Result<BatchCreateGrantResp, AppError> {
    let _ = (txn, actor_id, req);
    Err(AppError::Biz("未实现：batch_create_grants_in_tx".into()))
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
#[allow(dead_code)]
pub(crate) async fn fill_employee_names(
    db: &impl ConnectionTrait,
    employee_ids: &[u64],
) -> Result<HashMap<u64, String>, AppError> {
    let _ = (db, employee_ids);
    Err(AppError::Biz("未实现：fill_employee_names".into()))
}

/// 批量取假期类型名：`hr_leave_type.id` → `type_name`（单表批量查，排除软删行）。
// 实现提示：① 空入参返回空 map；② `repo::find_leave_types_by_ids(db, ids)` 单表批量查
// （已按 `DeletedAt.is_null()` 排除软删；repo 是唯一数据访问层，本域不另写 SQL）；
// ③ 回填 `id → type_name`；软删行不出现，调用方留空串。
// 骨架期：任务 4 的 api 层接入后删除本属性
#[allow(dead_code)]
pub(crate) async fn fill_leave_type_names(
    db: &impl ConnectionTrait,
    leave_type_ids: &[u64],
) -> Result<HashMap<u64, String>, AppError> {
    let _ = (db, leave_type_ids);
    Err(AppError::Biz("未实现：fill_leave_type_names".into()))
}

// —— 资源 CRUD：只读入口直连 db，自持事务的写入口走「三行事务」——

/// 假期类型分页：请求参数组装为 repo 过滤条件后透传（keyword 同时模糊编码与名称）。
// 实现提示：`repo::find_leave_type_page(db, &LeaveTypeFilter { keyword: req.keyword.clone(),
// status: req.status }, req.page.page_index(), req.page.page_size())`；`*_by_name` 由 api 层经
// `utils::user_ref::fill_user_names` 拼装，本层只回 `Model`。
pub async fn page_leave_types(
    db: &impl ConnectionTrait,
    req: &LeaveTypeListReq,
) -> Result<PageData<hr_leave_type::Model>, AppError> {
    let _ = (db, req);
    Err(AppError::Biz("未实现：page_leave_types".into()))
}

/// 创建假期类型（对外入口）：三行事务，成功后提交。
// 实现提示：`let txn = db.begin().await?;` → ① 编码查重
// `repo::find_leave_type_by_code_include_deleted(&txn, &req.type_code)` 命中即
// `AppError::Biz(format!("类型编码已存在：{}", req.type_code))`（软删行仍占唯一键，必须查含软删）；
// ② 组装 `hr_leave_type::ActiveModel`（业务列全量 Set）→
// `repo::create_leave_type_in_tx(&txn, model, actor_id)`；③ `txn.commit().await?`
// （`?` 冒泡时事务 Drop 自动回滚，无需显式 rollback）；返回值原样回传。
pub async fn create_leave_type(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateLeaveTypeReq,
) -> Result<hr_leave_type::Model, AppError> {
    let _ = (db, actor_id, req);
    Err(AppError::Biz("未实现：create_leave_type".into()))
}

/// 更新假期类型（对外入口）：三行事务，成功后提交。
// 实现提示：① `repo::find_leave_type_by_id(&txn, req.id)` 判存在（软删视为不存在，
// 不存在 → `AppError::Biz(format!("假期类型不存在：{}", req.id))`）；② 编码查重**排除自身**
// （`find_leave_type_by_code_include_deleted` 命中且 `id != req.id` 才报「类型编码已存在」）；
// ③ `repo::update_leave_type_in_tx(&txn, ActiveModel { id: Set(req.id), ..业务列 Set }, actor_id)`
// （窄写，`created_by` 保持 NotSet）；④ commit。
pub async fn update_leave_type(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateLeaveTypeReq,
) -> Result<hr_leave_type::Model, AppError> {
    let _ = (db, actor_id, req);
    Err(AppError::Biz("未实现：update_leave_type".into()))
}

/// 按 id 查假期类型详情（软删视为不存在）。
// 实现提示：`repo::find_leave_type_by_id(db, id)` → `None` 即
// `AppError::Biz(format!("假期类型不存在：{id}"))`；人名由 api 层经 `fill_user_names` 拼装。
pub async fn get_leave_type(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_leave_type::Model, AppError> {
    let _ = (db, id);
    Err(AppError::Biz("未实现：get_leave_type".into()))
}

/// 删除假期类型（对外入口）：软删，三行事务，成功后提交。
// 实现提示：① 引用检查（是否已有批次挂在该假别上）——建议 `repo::find_grant_page(&txn,
// &LeaveGrantFilter { leave_type_id: Some(id), ..Default::default() }, 0, 1)` 非空即
// `AppError::Biz("该假期类型已有额度批次，不能删除")`（口径由产品定，也可改为允许删除）；
// ② `repo::soft_delete_leave_type_in_tx(&txn, id, actor_id)` 返回 `false`（不存在 / 已软删）即
// `AppError::Biz(format!("假期类型不存在：{id}"))`；③ commit。
pub async fn delete_leave_type(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let _ = (db, actor_id, id);
    Err(AppError::Biz("未实现：delete_leave_type".into()))
}

/// 额度批次分页：请求参数组装为 repo 过滤条件后透传。
// 实现提示：`repo::find_grant_page(db, &LeaveGrantFilter { employee_id: req.employee_id,
// leave_type_id: req.leave_type_id, reason: req.reason.clone(), period: req.period.clone(),
// status: req.status }, req.page.page_index(), req.page.page_size())`；`employee_name` /
// `leave_type_name` 由 api 层经 [`fill_employee_names`] / [`fill_leave_type_names`] 批量回填。
pub async fn page_leave_grants(
    db: &impl ConnectionTrait,
    req: &LeaveGrantListReq,
) -> Result<PageData<hr_leave_grant::Model>, AppError> {
    let _ = (db, req);
    Err(AppError::Biz("未实现：page_leave_grants".into()))
}

/// 批量发放额度（对外入口）：三行事务，成功后提交。
// 实现提示：`let txn = db.begin().await?;` → `batch_create_grants_in_tx(&txn, actor_id, req)` →
// 成功 `txn.commit().await?` → 结果原样回传（部分员工命中幂等不算失败，走 `skipped` 回执）。
pub async fn batch_create_grants(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &BatchCreateGrantReq,
) -> Result<BatchCreateGrantResp, AppError> {
    let _ = (db, actor_id, req);
    Err(AppError::Biz("未实现：batch_create_grants".into()))
}

/// 按 id 查额度批次详情。
// 实现提示：`repo::find_grant_by_id(db, id)` → `None` 即
// `AppError::Biz(format!("额度批次不存在：{id}"))`；人名回填同上。
pub async fn get_leave_grant(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_leave_grant::Model, AppError> {
    let _ = (db, id);
    Err(AppError::Biz("未实现：get_leave_grant".into()))
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
    let _ = (db, actor_id, id);
    Err(AppError::Biz("未实现：cancel_grant".into()))
}

/// 额度账户分页：请求参数组装为 repo 过滤条件后透传。
// 实现提示：`repo::find_balance_page(db, &LeaveBalanceFilter { employee_id: req.employee_id,
// leave_type_id: req.leave_type_id, period: req.period.clone() }, req.page.page_index(),
// req.page.page_size())`；人名由 api 层经 [`fill_employee_names`] / [`fill_leave_type_names`] 回填。
pub async fn page_leave_balances(
    db: &impl ConnectionTrait,
    req: &LeaveBalanceListReq,
) -> Result<PageData<hr_leave_balance::Model>, AppError> {
    let _ = (db, req);
    Err(AppError::Biz("未实现：page_leave_balances".into()))
}

/// 按 id 查额度账户详情。
// 实现提示：`repo::find_balance_by_id(db, id)` → `None` 即
// `AppError::Biz(format!("额度账户不存在：{id}"))`；人名回填同上。
pub async fn get_leave_balance(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_leave_balance::Model, AppError> {
    let _ = (db, id);
    Err(AppError::Biz("未实现：get_leave_balance".into()))
}

/// 额度流水分页：请求参数组装为 repo 过滤条件后透传（append-only 对账凭据）。
// 实现提示：`repo::find_balance_log_page(db, &LeaveBalanceLogFilter { employee_id: req.employee_id,
// leave_type_id: req.leave_type_id, biz_type: req.biz_type }, req.page.page_index(),
// req.page.page_size())`；`employee_name` / `leave_type_name` / `operator_name` 由 api 层回填
// （`operator_name` 走 `utils::user_ref` 唯一管道）。
pub async fn page_leave_balance_logs(
    db: &impl ConnectionTrait,
    req: &LeaveBalanceLogListReq,
) -> Result<PageData<hr_leave_balance_log::Model>, AppError> {
    let _ = (db, req);
    Err(AppError::Biz("未实现：page_leave_balance_logs".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::hr_employee;
    use crate::modules::biz::hr::leave::dto::{BatchCreateGrantReq, LeaveBalanceLogFilter};
    use crate::modules::biz::hr::leave::{
        GRANT_SOURCE_ISSUE, GRANT_SOURCE_MANUAL, LOG_BIZ_GRANT, LOG_BIZ_LEAVE_RELEASE,
    };
    use crate::modules::biz::hr::leave::{repo, service};
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同进程内并行用例必须互不相同，否则撞 `uk_hr_leave_type_code`。
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
    fn leave_type_model(type_code: String) -> hr_leave_type::ActiveModel {
        hr_leave_type::ActiveModel {
            type_code: Set(type_code),
            type_name: Set("测试假别".to_owned()),
            unit: Set(1),
            balance_mode: Set(1),
            min_unit_minutes: Set(240),
            status: Set(1),
            ..Default::default()
        }
    }

    /// 直插一份员工档案 + 建一个假期类型，返回 (employee_id, leave_type_id)。
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

        let mut model = leave_type_model(unique("lt_service"));
        model.allow_negative = Set(allow_negative);
        let leave_type = repo::create_leave_type_in_tx(txn, model, ACTOR_ID)
            .await
            .unwrap();

        (employee.id, leave_type.id)
    }

    #[tokio::test]
    async fn grant_leave_creates_batch_and_account_and_log_in_one_call() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let grant_id = service::grant_leave_in_tx(
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
            &LeaveBalanceLogFilter {
                employee_id: Some(emp),
                ..Default::default()
            },
            1,
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
    async fn grant_leave_is_idempotent_for_same_reason_and_period() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type(&txn).await;
        let first = service::grant_leave_in_tx(
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
        let second = service::grant_leave_in_tx(
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
        let expiring = service::grant_leave_in_tx(
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
        let _fresh = service::grant_leave_in_tx(
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
        service::lock_leave_in_tx(&txn, emp, ty, 480, date(2026, 2, 1))
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
    async fn lock_leave_rejects_when_available_is_insufficient_and_type_disallows_negative() {
        let txn = test_txn().await;
        let (emp, ty) = seed_employee_and_type_with(&txn, 0 /* allow_negative */).await;
        // 先发一笔**不足额**的额度（240 < 480）：账户存在才可能走到「可用不足」分支，
        // 否则命中的是「账户不存在」文案，断言就失去区分力。
        service::grant_leave_in_tx(
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
        let err = service::lock_leave_in_tx(&txn, emp, ty, 480, date(2026, 2, 1))
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
        service::grant_leave_in_tx(
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
        service::lock_leave_in_tx(&txn, emp, ty, 240, date(2026, 2, 1))
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
            &LeaveBalanceLogFilter {
                employee_id: Some(emp),
                biz_type: Some(LOG_BIZ_LEAVE_RELEASE),
                ..Default::default()
            },
            1,
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
        let stale = service::grant_leave_in_tx(
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
        // 2025 年生效、2025 年底失效的结转批次 → 入 2025 账期账户
        let _stale = service::grant_leave_in_tx(
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
        service::grant_leave_in_tx(
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
        service::grant_leave_in_tx(
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
        service::lock_leave_in_tx(&txn, emp, ty, 480, date(2026, 2, 1))
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
            &LeaveBalanceLogFilter {
                employee_id: Some(emp),
                ..Default::default()
            },
            1,
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
            leave_type_id: ty,
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
