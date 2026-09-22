//! 加班域业务：加班单 CRUD + 状态机（建单即提交 / 撤回 / 删除） + 审批终态副作用（调休入账）。
//!
//! 分层约定（见 AGENTS.md「分层契约」）：
//! - 自持事务的入口收 `db: &DatabaseConnection`，内部委托 `pub(crate) *_in_tx(txn, …)`；
//! - 只读入口收 `&impl ConnectionTrait`，不起事务；
//! - `repo` 只拼 SQL；值域校验在 `validate.rs`（api 层调用），**查库的规则**在本层：
//!   员工档案存在且未离职、加班类型与应出勤日是否匹配、与 `work_date` 是否同日、
//!   同日已通过加班单区间是否重叠。
//!
//! # 时长派生（设计 §4.2，R5）
//!
//! 请求体只收起止时间，`duration_minutes` 一律后端派生：`end_at − start_at` 的**分钟总数**，
//! **不裁剪到班次窗口**（加班本就发生在窗口之外）；派生只做校验——工作日加班不得与当日
//! 应工作窗口重叠、休息日 / 法定节假日加班要求当日**不是**应出勤日。
//! 应出勤窗口来自考勤域 `attendance::service::resolve_workday`（排班 × 工作日历实时派生），
//! 本域不自己判日历，避免两套口径。
//!
//! # 状态机
//!
//! ```text
//! create（建单即提交） → 1 审批中 ──审批通过──→ 2 已通过（comp_mode = 1 时同事务入调休）
//!                              ├──审批驳回──→ 3 已驳回 ──update / submit──→ 1 审批中
//!                              └──cancel───→ 4 已撤销 ──update / submit──→ 1 审批中
//! ```
//!
//! - `update` 仅允许 `3 / 4`（重新提交前先把内容改对）；
//! - `submit` 仅允许 `3 / 4`（重新起审批实例）；
//! - `cancel` 仅允许 `1`（先置 4 再撤在途实例，回调不会把它改写回 3）；
//! - `delete` 软删：审批中的单据先撤在途实例，其余直接软删。
//!
//! 并发：所有「读 → 判断 → 写」都走 `find_overtime_by_id_for_update`（单据行）与
//! `lock_employee_for_update`（员工行，串行化同一员工的并发建单与区间重叠判定）。

use std::collections::HashMap;

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime};
use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::{hr_employee, hr_overtime_request};
use crate::modules::biz::hr::approval::{BIZ_TYPE_OVERTIME, service as approval_service};
use crate::modules::biz::hr::attendance::service::{WorkdayWindow, resolve_workday};
use crate::modules::biz::hr::overtime::dto::{
    CreateOvertimeReq, MineReq, OvertimeFilter, OvertimeListReq, UpdateOvertimeReq,
};
use crate::modules::biz::hr::overtime::{
    COMP_MODE_TIME_OFF, OVERTIME_TYPE_HOLIDAY, OVERTIME_TYPE_REST_DAY, OVERTIME_TYPE_WORKDAY,
    REQUEST_STATUS_APPROVED, REQUEST_STATUS_CANCELED, REQUEST_STATUS_PENDING,
    REQUEST_STATUS_REJECTED, TIME_OFF_EXPIRE_MONTHS, repo as overtime_repo,
};
use crate::modules::biz::hr::time_off::{
    GRANT_REASON_OVERTIME, GRANT_SOURCE_OVERTIME, SOURCE_KIND_OVERTIME,
    TIME_OFF_TYPE_CODE_COMPENSATORY, service as time_off_service,
};
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 在职状态「离职」（字典 `employmentStatus` 的 3）——离职员工不能再报加班。
const EMPLOYMENT_STATUS_RESIGNED: i8 = 3;

// —— 解析 helper（`validate.rs` 已拦格式，这里是进入 service 后的兜底）——

/// 解析 `yyyy-MM-dd` 加班所属日期。
fn parse_work_date(raw: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::Biz(format!("加班日期格式错误：{raw}")))
}

/// 解析必填的 `yyyy-MM-dd HH:mm:ss`（同时兼容纯日期，落当日 00:00:00）。
fn parse_required_datetime(field: &str, raw: &str) -> Result<NaiveDateTime, AppError> {
    crate::utils::datetime::parse_datetime(field, &Some(raw.to_string()), false)?
        .ok_or_else(|| AppError::Biz(format!("{field}不能为空")))
}

/// 日期区间入参 → `NaiveDate`（`end_of_day` 决定纯日期止是否补到 23:59:59，DATE 列比较只取日期部分）。
fn parse_date_bound(
    field: &str,
    raw: &Option<String>,
    end_of_day: bool,
) -> Result<Option<NaiveDate>, AppError> {
    Ok(crate::utils::datetime::parse_datetime(field, raw, end_of_day)?.map(|dt| dt.date()))
}

/// `work_date` 加 `months` 个自然月：按月进位，**月末不足取当月最后一天**
/// （如 11-30 + 3 → 次年 2-28/29）。不用「+90 天」：那是三个月零几天，且随月份漂移。
fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    let total = date.year() * 12
        + i32::try_from(date.month0()).unwrap_or(0)
        + i32::try_from(months).unwrap_or(0);
    let (year, month0) = (total.div_euclid(12), total.rem_euclid(12));
    let month = u32::try_from(month0).map_or(1, |m| m + 1);
    let day = date.day().min(last_day_of_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).unwrap_or(date)
}

/// 某年某月的最后一天（28..=31 里第一个能在日历上落地的日）。
fn last_day_of_month(year: i32, month: u32) -> u32 {
    (28..=31)
        .rev()
        .find(|day| NaiveDate::from_ymd_opt(year, month, *day).is_some())
        .unwrap_or(28)
}

// —— 列表 / 详情（只读入口直连 db）——

/// 加班单分页：请求参数组装为 repo 过滤条件后透传（`work_date` 区间在 DTO 是 `String`，
/// 在这里解析；`*_by_name` / `employee_name` 由 api 层拼装，本层只回 `Model`）。
pub async fn page_overtimes(
    db: &impl ConnectionTrait,
    req: &OvertimeListReq,
) -> Result<PageData<hr_overtime_request::Model>, AppError> {
    let filter = OvertimeFilter {
        employee_id: req.employee_id,
        status: req.status,
        overtime_type: req.overtime_type,
        work_date_begin: parse_date_bound("加班日期范围起", &req.work_date_begin, false)?,
        work_date_end: parse_date_bound("加班日期范围止", &req.work_date_end, true)?,
    };
    Ok(
        overtime_repo::find_overtime_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 加班单详情（软删视为不存在）。
pub async fn get_overtime(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_overtime_request::Model, AppError> {
    overtime_repo::find_overtime_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("加班单不存在：{id}")))
}

/// 我的加班单（申请人自助视角）：`AuthUser.user_id` → 员工档案 → 只看自己的单据。
pub async fn page_my_overtimes(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &MineReq,
) -> Result<PageData<hr_overtime_request::Model>, AppError> {
    let employee = overtime_repo::find_employee_by_user_id(db, actor_id)
        .await?
        .ok_or_else(|| AppError::Biz("未找到当前用户的员工档案".into()))?;
    let filter = OvertimeFilter {
        employee_id: Some(employee.id),
        status: req.status,
        ..Default::default()
    };
    Ok(
        overtime_repo::find_overtime_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

// —— 写入口：自持事务的「三行事务」+ 可复用的 `_in_tx` 实现 ——

/// 创建加班单并**直接提交**审批（建单即提交，无草稿态）。
pub async fn create_overtime(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateOvertimeReq,
) -> Result<hr_overtime_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let model = create_overtime_in_tx(&txn, actor_id, &req).await?;
    txn.commit().await.map_err(anyhow::Error::from)?;
    Ok(model)
}

/// 事务内创建加班单：校验 → 派生时长 → 落库 → 起审批实例 → 回写实例 ID 与状态。
pub(crate) async fn create_overtime_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateOvertimeReq,
) -> Result<hr_overtime_request::Model, AppError> {
    let work_date = parse_work_date(&req.work_date)?;
    let start_at = parse_required_datetime("加班开始时间", &req.start_at)?;
    let end_at = parse_required_datetime("加班结束时间", &req.end_at)?;

    // 先锁员工行：区间重叠判定是「读 → 判断 → 写」，同一员工的并发建单必须串行化
    let employee = lock_active_employee_in_tx(txn, req.employee_id).await?;
    let duration_minutes = validate_and_derive_in_tx(
        txn,
        &employee,
        work_date,
        start_at,
        end_at,
        req.overtime_type,
    )
    .await?;

    let created = overtime_repo::create_overtime_in_tx(
        txn,
        hr_overtime_request::ActiveModel {
            employee_id: Set(req.employee_id),
            work_date: Set(work_date),
            start_at: Set(start_at),
            end_at: Set(end_at),
            duration_minutes: Set(duration_minutes),
            overtime_type: Set(req.overtime_type),
            comp_mode: Set(req.comp_mode),
            reason: Set(req.reason.trim().to_string()),
            attachment_id: Set(req.attachment_id),
            status: Set(REQUEST_STATUS_PENDING),
            approval_instance_id: Set(0),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    // 建单即提交：同事务内起审批实例（申请人 = 单据所属员工的账号），把实例 ID 写回
    let instance_id = approval_service::start_instance_in_tx(
        txn,
        actor_id,
        BIZ_TYPE_OVERTIME,
        created.id,
        employee.user_id,
    )
    .await?;

    Ok(overtime_repo::update_overtime_in_tx(
        txn,
        hr_overtime_request::ActiveModel {
            id: Set(created.id),
            approval_instance_id: Set(instance_id),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 修改加班单（仅「已驳回 / 已撤销」可改，改完需重新提交）。
pub async fn update_overtime(
    db: &DatabaseConnection,
    actor_id: u64,
    req: UpdateOvertimeReq,
) -> Result<hr_overtime_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let model = update_overtime_in_tx(&txn, actor_id, &req).await?;
    txn.commit().await.map_err(anyhow::Error::from)?;
    Ok(model)
}

/// 事务内修改加班单：判状态 → 同创建口径的重校验与时长重派生 → 窄写（状态不变）。
pub(crate) async fn update_overtime_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateOvertimeReq,
) -> Result<hr_overtime_request::Model, AppError> {
    let existing = overtime_repo::find_overtime_by_id_for_update(txn, req.id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("加班单不存在：{}", req.id)))?;
    if !matches!(
        existing.status,
        REQUEST_STATUS_REJECTED | REQUEST_STATUS_CANCELED
    ) {
        return Err(AppError::Biz("已提交或已通过的加班单不能修改".into()));
    }

    let work_date = parse_work_date(&req.work_date)?;
    let start_at = parse_required_datetime("加班开始时间", &req.start_at)?;
    let end_at = parse_required_datetime("加班结束时间", &req.end_at)?;

    let employee = lock_active_employee_in_tx(txn, req.employee_id).await?;
    let duration_minutes = validate_and_derive_in_tx(
        txn,
        &employee,
        work_date,
        start_at,
        end_at,
        req.overtime_type,
    )
    .await?;

    Ok(overtime_repo::update_overtime_in_tx(
        txn,
        hr_overtime_request::ActiveModel {
            id: Set(req.id),
            employee_id: Set(req.employee_id),
            work_date: Set(work_date),
            start_at: Set(start_at),
            end_at: Set(end_at),
            duration_minutes: Set(duration_minutes),
            overtime_type: Set(req.overtime_type),
            comp_mode: Set(req.comp_mode),
            reason: Set(req.reason.trim().to_string()),
            attachment_id: Set(req.attachment_id),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 重新提交加班单（仅「已驳回 / 已撤销」）。
pub async fn submit_overtime(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<hr_overtime_request::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let model = submit_overtime_in_tx(&txn, actor_id, id).await?;
    txn.commit().await.map_err(anyhow::Error::from)?;
    Ok(model)
}

/// 事务内重新提交：先起审批实例，再回写实例 ID 与「审批中」。
pub(crate) async fn submit_overtime_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<hr_overtime_request::Model, AppError> {
    let existing = overtime_repo::find_overtime_by_id_for_update(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("加班单不存在：{id}")))?;
    if !matches!(
        existing.status,
        REQUEST_STATUS_REJECTED | REQUEST_STATUS_CANCELED
    ) {
        return Err(AppError::Biz(
            "只有已驳回或已撤销的加班单可以重新提交".into(),
        ));
    }

    let employee = overtime_repo::find_employee_by_id(txn, existing.employee_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("员工档案不存在：{}", existing.employee_id)))?;
    if employee.employment_status == EMPLOYMENT_STATUS_RESIGNED {
        return Err(AppError::Biz(format!(
            "员工已离职，不能提交加班单：{}",
            existing.employee_id
        )));
    }

    let instance_id = approval_service::start_instance_in_tx(
        txn,
        actor_id,
        BIZ_TYPE_OVERTIME,
        existing.id,
        employee.user_id,
    )
    .await?;

    Ok(overtime_repo::update_overtime_in_tx(
        txn,
        hr_overtime_request::ActiveModel {
            id: Set(existing.id),
            approval_instance_id: Set(instance_id),
            status: Set(REQUEST_STATUS_PENDING),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 撤销加班单（仅「审批中」）。
pub async fn cancel_overtime(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    cancel_overtime_in_tx(&txn, actor_id, id).await?;
    txn.commit().await.map_err(anyhow::Error::from)?;
    Ok(())
}

/// 事务内撤销：**先**把单据置「已撤销」再撤在途实例——审批侧终态回调看到单据已是终态，
/// 不会把它改写成「已驳回」（见 [`on_instance_finished_in_tx`]）。
pub(crate) async fn cancel_overtime_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let existing = overtime_repo::find_overtime_by_id_for_update(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("加班单不存在：{id}")))?;
    if existing.status != REQUEST_STATUS_PENDING {
        return Err(AppError::Biz("只有审批中的加班单可以撤销".into()));
    }

    overtime_repo::update_overtime_in_tx(
        txn,
        hr_overtime_request::ActiveModel {
            id: Set(existing.id),
            status: Set(REQUEST_STATUS_CANCELED),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    approval_service::cancel_by_biz_in_tx(txn, actor_id, BIZ_TYPE_OVERTIME, existing.id).await
}

/// 删除加班单（软删）。
pub async fn delete_overtime(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    delete_overtime_in_tx(&txn, actor_id, id).await?;
    txn.commit().await.map_err(anyhow::Error::from)?;
    Ok(())
}

/// 事务内删除（软删）：审批中的单据先撤在途实例（否则实例会永远悬着），其余直接软删。
pub(crate) async fn delete_overtime_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let existing = overtime_repo::find_overtime_by_id_for_update(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("加班单不存在：{id}")))?;

    if existing.status == REQUEST_STATUS_PENDING {
        // 审批中的单据先撤在途实例，否则实例会永远悬着
        approval_service::cancel_by_biz_in_tx(txn, actor_id, BIZ_TYPE_OVERTIME, existing.id)
            .await?;
    }

    if !overtime_repo::soft_delete_overtime_in_tx(txn, id, actor_id).await? {
        return Err(AppError::Biz(format!("加班单不存在：{id}")));
    }
    Ok(())
}

// —— 审批终态回调（由 hr/approval 按 biz_type = overtime 分派，同事务）——

/// 审批终态回调：通过 → 置「已通过」，`comp_mode = 1 转调休` 时**同事务**生成调休批次；
/// 驳回 / 撤销 → 置「已驳回」（撤销态由调用方先置位，这里不改写）。
///
/// **幂等**：单据已是终态（2 / 3 / 4）直接返回——重复回调不会重复入账；
/// 调休批次本身还有一层来源幂等键（`source_kind = 3` + `source_id = 加班单 ID`）兜底。
/// `comp_mode = 2 计加班费` 只置状态（P5 薪酬不做金额）。
pub(crate) async fn on_instance_finished_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    biz_id: u64,
    approved: bool,
) -> Result<(), AppError> {
    let request = overtime_repo::find_overtime_by_id_for_update(txn, biz_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("加班单不存在：{biz_id}")))?;
    if request.status != REQUEST_STATUS_PENDING {
        // 已终结（含申请人撤销）：不改写状态、不再入账
        return Ok(());
    }

    if !approved {
        overtime_repo::update_overtime_in_tx(
            txn,
            hr_overtime_request::ActiveModel {
                id: Set(request.id),
                status: Set(REQUEST_STATUS_REJECTED),
                ..Default::default()
            },
            actor_id,
        )
        .await?;
        return Ok(());
    }

    overtime_repo::update_overtime_in_tx(
        txn,
        hr_overtime_request::ActiveModel {
            id: Set(request.id),
            status: Set(REQUEST_STATUS_APPROVED),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    if request.comp_mode != COMP_MODE_TIME_OFF {
        return Ok(());
    }

    // 调休入账：假别 ID 由 type_code 查库得到（不写死数字），有效期 = work_date + 3 个自然月
    let time_off_type_id = compensatory_type_id_in_tx(txn).await?;
    let period = request.work_date.year().to_string();
    let expire_at = add_months(request.work_date, TIME_OFF_EXPIRE_MONTHS);

    time_off_service::grant_time_off_in_tx(
        txn,
        actor_id,
        request.employee_id,
        time_off_type_id,
        request.duration_minutes,
        GRANT_SOURCE_OVERTIME,
        GRANT_REASON_OVERTIME,
        &period,
        request.work_date,
        Some(expire_at),
        SOURCE_KIND_OVERTIME,
        request.id,
    )
    .await?;

    Ok(())
}

// —— 私有 helper：把「校验」这件事收口一次，创建 / 修改共用 ——

/// 加锁读员工档案并校验「存在且未离职」（离职员工不能再报加班）。
async fn lock_active_employee_in_tx(
    txn: &DatabaseTransaction,
    employee_id: u64,
) -> Result<hr_employee::Model, AppError> {
    let employee = overtime_repo::lock_employee_for_update(txn, employee_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("员工档案不存在：{employee_id}")))?;
    if employee.employment_status == EMPLOYMENT_STATUS_RESIGNED {
        return Err(AppError::Biz(format!(
            "员工已离职，不能提交加班单：{employee_id}"
        )));
    }
    Ok(employee)
}

/// 校验加班区间并派生时长（分钟）：返回 `duration_minutes`，不做任何写操作。
///
/// 规则（顺序即文案优先级）：
/// 1. 起止时间必须都落在 `work_date` 当天（跨天加班请拆单）；
/// 2. `1 工作日` → 该日必须是应出勤日，且区间不得与当日应工作窗口重叠；
///    `2 休息日` / `3 法定节假日` → 该日必须**不是**应出勤日；
/// 3. 同员工同 `work_date` 已通过的加班单区间不得重叠；
/// 4. 时长 = 区间总长（**不裁剪到班次窗口**）。
async fn validate_and_derive_in_tx(
    txn: &DatabaseTransaction,
    employee: &hr_employee::Model,
    work_date: NaiveDate,
    start_at: NaiveDateTime,
    end_at: NaiveDateTime,
    overtime_type: i8,
) -> Result<i32, AppError> {
    if start_at.date() != work_date || end_at.date() != work_date {
        return Err(AppError::Biz(format!(
            "加班开始与结束时间必须落在加班日期 {work_date} 当天（跨天加班请拆分为多条单据）"
        )));
    }
    if end_at <= start_at {
        return Err(AppError::Biz("加班结束时间必须晚于开始时间".into()));
    }

    // 应出勤窗口由考勤域按「排班 × 工作日历」实时派生，本域不自己判日历
    let window = resolve_workday(txn, employee.id, work_date).await?;
    match overtime_type {
        OVERTIME_TYPE_WORKDAY => {
            if !window.is_workday {
                return Err(AppError::Biz("该日不是工作日，请选择正确的加班类型".into()));
            }
            if let Some((window_start, window_end)) = work_window_bounds(work_date, &window)
                && start_at < window_end
                && end_at > window_start
            {
                return Err(AppError::Biz(
                    "工作日加班不能与应工作时段重叠，请调整加班时间".into(),
                ));
            }
        }
        OVERTIME_TYPE_REST_DAY | OVERTIME_TYPE_HOLIDAY => {
            if window.is_workday {
                return Err(AppError::Biz("该日不是休息日，请选择正确的加班类型".into()));
            }
        }
        other => return Err(AppError::Biz(format!("未知的加班类型：{other}"))),
    }

    let overlaps = overtime_repo::find_overlapping_approved_overtime(
        txn,
        employee.id,
        work_date,
        start_at,
        end_at,
    )
    .await?;
    if let Some(first) = overlaps.first() {
        return Err(AppError::Biz(format!(
            "该员工当日已有通过的加班单（单据编号：{}）与本次区间重叠",
            first.id
        )));
    }

    i32::try_from((end_at - start_at).num_minutes())
        .map_err(|_| AppError::Biz("加班时长超出可记录范围".into()))
}

/// 应工作窗口的绝对时间边界 `(开始, 结束)`；无窗口或窗口不成区间时返回 `None`
/// （无排班的工作日只有 `standard_minutes`，没有可比的窗口）。跨天班次的结束落在次日。
fn work_window_bounds(
    work_date: NaiveDate,
    window: &WorkdayWindow,
) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let (start, end) = (window.start_time?, window.end_time?);
    let window_start = work_date.and_time(start);
    let window_end = if window.cross_day {
        work_date.checked_add_days(Days::new(1))?.and_time(end)
    } else {
        work_date.and_time(end)
    };
    (window_end > window_start).then_some((window_start, window_end))
}

/// 调休假别 ID：按种子编码 `type_code = comp` 查启用假别（不写死数字）。
async fn compensatory_type_id_in_tx(txn: &DatabaseTransaction) -> Result<u64, AppError> {
    overtime_repo::find_enabled_time_off_type_by_code(txn, TIME_OFF_TYPE_CODE_COMPENSATORY)
        .await?
        .map(|t| t.id)
        .ok_or_else(|| AppError::Biz("未配置调休假别（type_code = comp），请联系管理员".into()))
}

// —— 批量取名 helper（列表 / 详情响应用；单次查询，禁止逐行查库）——

/// 批量取员工名：`hr_employee.id` → `sys_user.username`。
///
/// 与平台唯一拼名管道口径一致（映射 `sys_user.username`）：先一次批量取档案拿 `user_id`，
/// 再交给 `utils::user_ref::find_user_name_map_by_ids` 一次批量查名；软删档案 / 用户不出现在
/// 映射里，调用方留空串。**不得逐行查库**。
pub async fn fill_employee_names(
    db: &impl ConnectionTrait,
    employee_ids: &[u64],
) -> Result<HashMap<u64, String>, AppError> {
    let ids = crate::utils::user_ref::dedup_ids(employee_ids.to_vec());
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let employees = overtime_repo::find_employees_by_ids(db, &ids).await?;
    let user_ids = employees.iter().map(|e| e.user_id).collect();
    let names = crate::utils::user_ref::find_user_name_map_by_ids(db, user_ids).await?;
    Ok(employees
        .into_iter()
        .filter_map(|e| names.get(&e.user_id).cloned().map(|name| (e.id, name)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{hr_shift, hr_shift_schedule, hr_time_off_grant, hr_time_off_type};
    use crate::modules::biz::hr::overtime::COMP_MODE_PAY;
    use chrono::NaiveTime;
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同进程内并行用例必须互不相同（班次编码 `uk_hr_shift_shift_code` 单列唯一）。
    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一员工 ID：段位 900_6xx，与 time_off 域测试（900_3xx）、employee 域测试（900_1xx）
    /// 错开，避免同进程并行撞 `uk_hr_employee_user_id`。
    fn unique_employee_id() -> u64 {
        900_600_000
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed) % 1_000
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn dt(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> NaiveDateTime {
        date(y, m, d).and_hms_opt(hour, minute, 0).unwrap()
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

    /// 直插一份在职员工档案（`hr_employee` 非空列都有 DDL 默认值）。
    async fn seed_employee(txn: &DatabaseTransaction) -> hr_employee::Model {
        hr_employee::ActiveModel {
            user_id: Set(unique_employee_id()),
            employment_status: Set(1),
            education: Set(0),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap()
    }

    /// 造「应出勤日」：排班挂一个 09:00–18:00 的班次（`resolve_workday` 由此给出窗口与标准工时）。
    async fn seed_workday(txn: &DatabaseTransaction, employee_id: u64, work_date: NaiveDate) {
        let shift = hr_shift::ActiveModel {
            shift_code: Set(unique("ot_shift")),
            shift_name: Set("测试白班".to_owned()),
            start_time: Set(NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
            end_time: Set(NaiveTime::from_hms_opt(18, 0, 0).unwrap()),
            cross_day: Set(0),
            work_minutes: Set(480),
            rest_minutes: Set(60),
            late_tolerance_minutes: Set(0),
            need_clock: Set(1),
            status: Set(1),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap();
        seed_schedule(txn, employee_id, work_date, shift.id).await;
    }

    /// 造「休息日」：排班显式 `shift_id = 0`（排班覆盖日历，考勤域口径）。
    async fn seed_rest_day(txn: &DatabaseTransaction, employee_id: u64, work_date: NaiveDate) {
        seed_schedule(txn, employee_id, work_date, 0).await;
    }

    async fn seed_schedule(
        txn: &DatabaseTransaction,
        employee_id: u64,
        work_date: NaiveDate,
        shift_id: u64,
    ) {
        hr_shift_schedule::ActiveModel {
            employee_id: Set(employee_id),
            work_date: Set(work_date),
            shift_id: Set(shift_id),
            status: Set(1),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap();
    }

    /// 调休假别（种子 `type_code = comp`）：存在即用，不存在则补一条（测试事务回滚不留痕）。
    async fn seed_comp_type(txn: &DatabaseTransaction) -> u64 {
        if let Some(existing) =
            overtime_repo::find_enabled_time_off_type_by_code(txn, TIME_OFF_TYPE_CODE_COMPENSATORY)
                .await
                .unwrap()
        {
            return existing.id;
        }
        // 与并行进程的补种撞单列唯一键：重读对方刚插入的行
        let inserted = hr_time_off_type::ActiveModel {
            type_code: Set(TIME_OFF_TYPE_CODE_COMPENSATORY.to_string()),
            type_name: Set("调休".to_owned()),
            unit: Set(2),
            balance_mode: Set(1),
            min_unit_minutes: Set(60),
            status: Set(1),
            ..Default::default()
        }
        .insert(txn)
        .await;
        match inserted {
            Ok(model) => model.id,
            Err(_) => {
                overtime_repo::find_enabled_time_off_type_by_code(
                    txn,
                    TIME_OFF_TYPE_CODE_COMPENSATORY,
                )
                .await
                .unwrap()
                .expect("调休假别应存在")
                .id
            }
        }
    }

    /// 直插一张加班单（终态回调用例只关心派生副作用，绕过审批基座）。
    async fn seed_request(
        txn: &DatabaseTransaction,
        employee_id: u64,
        work_date: NaiveDate,
        comp_mode: i8,
        status: i8,
        duration_minutes: i32,
    ) -> hr_overtime_request::Model {
        overtime_repo::create_overtime_in_tx(
            txn,
            hr_overtime_request::ActiveModel {
                employee_id: Set(employee_id),
                work_date: Set(work_date),
                start_at: Set(dt(
                    work_date.year(),
                    work_date.month(),
                    work_date.day(),
                    19,
                    0,
                )),
                end_at: Set(dt(
                    work_date.year(),
                    work_date.month(),
                    work_date.day(),
                    21,
                    30,
                )),
                duration_minutes: Set(duration_minutes),
                overtime_type: Set(OVERTIME_TYPE_REST_DAY),
                comp_mode: Set(comp_mode),
                reason: Set("测试加班".to_owned()),
                status: Set(status),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap()
    }

    /// 走「建单即提交」的完整入口构造请求（校验失败即返回 `Err`，不会走到起审批实例）。
    fn create_req(
        employee_id: u64,
        work_date: NaiveDate,
        overtime_type: i8,
        start: (u32, u32),
        end: (u32, u32),
    ) -> CreateOvertimeReq {
        let fmt = |(hour, minute): (u32, u32)| {
            format!(
                "{} {:02}:{:02}:00",
                work_date.format("%Y-%m-%d"),
                hour,
                minute
            )
        };
        CreateOvertimeReq {
            employee_id,
            work_date: work_date.format("%Y-%m-%d").to_string(),
            start_at: fmt(start),
            end_at: fmt(end),
            overtime_type,
            comp_mode: COMP_MODE_TIME_OFF,
            reason: "版本上线".to_owned(),
            attachment_id: 0,
            remark: String::new(),
        }
    }

    /// 该员工的全部调休批次（按 id 升序）。
    async fn grants_of(
        db: &impl ConnectionTrait,
        employee_id: u64,
    ) -> Vec<hr_time_off_grant::Model> {
        hr_time_off_grant::Entity::find()
            .filter(hr_time_off_grant::Column::EmployeeId.eq(employee_id))
            .all(db)
            .await
            .unwrap()
    }

    /// ① 工作日类型落在休息日（排班 `shift_id = 0`）→ 拒。
    #[tokio::test]
    async fn workday_type_on_rest_day_is_rejected() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 10);
        seed_rest_day(&txn, employee.id, work_date).await;

        let err = create_overtime_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(
                employee.id,
                work_date,
                OVERTIME_TYPE_WORKDAY,
                (19, 0),
                (21, 0),
            ),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, AppError::Biz(m) if m.contains("该日不是工作日")),
            "工作日类型落在休息日必须被拒：{err:?}"
        );
    }

    /// ① 反向：休息日类型落在应出勤日（排班挂 09:00–18:00 班次）→ 拒。
    #[tokio::test]
    async fn rest_day_type_on_workday_is_rejected() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 11);
        seed_workday(&txn, employee.id, work_date).await;

        let err = create_overtime_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(
                employee.id,
                work_date,
                OVERTIME_TYPE_REST_DAY,
                (19, 0),
                (21, 0),
            ),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, AppError::Biz(m) if m.contains("该日不是休息日")),
            "休息日类型落在应出勤日必须被拒：{err:?}"
        );
    }

    /// ① 工作日类型 + 区间压在应工作窗口内 → 拒（只有类型匹配不够，时段也必须错开）。
    #[tokio::test]
    async fn workday_type_overlapping_shift_window_is_rejected() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 12);
        seed_workday(&txn, employee.id, work_date).await; // 窗口 09:00–18:00

        let err = create_overtime_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(
                employee.id,
                work_date,
                OVERTIME_TYPE_WORKDAY,
                (10, 0),
                (12, 0),
            ),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, AppError::Biz(m) if m.contains("不能与应工作时段重叠")),
            "工作日加班与应工作窗口重叠必须被拒：{err:?}"
        );
    }

    /// 跨天区间直接拒绝（单据以班次开始日为准，本域不接受跨天提交）。
    #[tokio::test]
    async fn cross_day_interval_is_rejected() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 13);
        seed_rest_day(&txn, employee.id, work_date).await;

        let mut req = create_req(
            employee.id,
            work_date,
            OVERTIME_TYPE_REST_DAY,
            (22, 0),
            (23, 0),
        );
        req.end_at = "2097-03-14 02:00:00".to_string();

        let err = create_overtime_in_tx(&txn, ACTOR_ID, &req)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AppError::Biz(m) if m.contains("跨天")),
            "跨天加班应被拒并提示拆单：{err:?}"
        );
    }

    /// 时长派生 = 区间总长，**不裁剪到班次窗口**（19:00–21:30 在 09:00–18:00 窗口之外，仍是 150 分钟）。
    #[tokio::test]
    async fn duration_is_interval_length_and_not_clipped_to_shift_window() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 14);
        seed_workday(&txn, employee.id, work_date).await;

        let minutes = validate_and_derive_in_tx(
            &txn,
            &employee,
            work_date,
            dt(2097, 3, 14, 19, 0),
            dt(2097, 3, 14, 21, 30),
            OVERTIME_TYPE_WORKDAY,
        )
        .await
        .unwrap();
        assert_eq!(
            minutes, 150,
            "加班时长应取区间总长（19:00–21:30 = 150 分钟）"
        );
    }

    /// ② 同日已通过加班单区间重叠 → 拒。
    #[tokio::test]
    async fn overlapping_approved_overtime_on_same_day_is_rejected() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 15);
        seed_rest_day(&txn, employee.id, work_date).await;
        // 当日已有一条 19:00–21:30 的已通过加班单
        seed_request(
            &txn,
            employee.id,
            work_date,
            COMP_MODE_PAY,
            REQUEST_STATUS_APPROVED,
            150,
        )
        .await;

        let err = create_overtime_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(
                employee.id,
                work_date,
                OVERTIME_TYPE_REST_DAY,
                (20, 0),
                (22, 0),
            ),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, AppError::Biz(m) if m.contains("区间重叠")),
            "同日已通过加班单区间重叠必须被拒：{err:?}"
        );

        // 不重叠的时段仍然可提交（校验只挡相交，不是「一天只能一条」）
        let ok = validate_and_derive_in_tx(
            &txn,
            &employee,
            work_date,
            dt(2097, 3, 15, 21, 30),
            dt(2097, 3, 15, 23, 0),
            OVERTIME_TYPE_REST_DAY,
        )
        .await;
        assert!(ok.is_ok(), "不重叠区间不应被拒：{ok:?}");
    }

    /// ③ `comp_mode = 1` 通过 → 生成调休批次（来源类型 / 来源单据 / 有效期），重复回调不再入账。
    #[tokio::test]
    async fn approved_time_off_mode_grants_compensatory_batch_once() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let comp_type_id = seed_comp_type(&txn).await;
        let work_date = date(2097, 3, 16);
        let request = seed_request(
            &txn,
            employee.id,
            work_date,
            COMP_MODE_TIME_OFF,
            REQUEST_STATUS_PENDING,
            150,
        )
        .await;

        on_instance_finished_in_tx(&txn, ACTOR_ID, request.id, true)
            .await
            .unwrap();

        let updated = overtime_repo::find_overtime_by_id(&txn, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status, REQUEST_STATUS_APPROVED,
            "审批通过后加班单应置「已通过」"
        );

        let grants = grants_of(&txn, employee.id).await;
        assert_eq!(grants.len(), 1, "转调休通过后应生成一条调休批次");
        assert_eq!(
            grants[0].source_kind, SOURCE_KIND_OVERTIME,
            "调休批次的来源类型应为「加班单」"
        );
        assert_eq!(
            grants[0].source_id, request.id,
            "调休批次的来源单据应为加班单 ID"
        );
        assert_eq!(
            grants[0].source, GRANT_SOURCE_OVERTIME,
            "批次来源应为「加班转调休」"
        );
        assert_eq!(
            grants[0].time_off_type_id, comp_type_id,
            "批次应落进调休假别（type_code = comp）"
        );
        assert_eq!(grants[0].minutes, 150, "入账分钟数应为加班时长");
        assert_eq!(grants[0].remaining_minutes, 150, "新批次剩余应等于授予量");
        assert_eq!(
            grants[0].reason, GRANT_REASON_OVERTIME,
            "发放依据应为「加班转调休」（字典 timeOffGrantReason）"
        );
        assert_eq!(grants[0].period, "2097", "归属周期应为加班所属年份");
        assert_eq!(grants[0].effective_at, work_date, "生效日应为加班所属日期");
        assert_eq!(
            grants[0].expire_at,
            Some(date(2097, 6, 16)),
            "失效日应为加班日 + 3 个自然月"
        );

        // 幂等：重复回调不再生成批次
        on_instance_finished_in_tx(&txn, ACTOR_ID, request.id, true)
            .await
            .unwrap();
        assert_eq!(
            grants_of(&txn, employee.id).await.len(),
            1,
            "重复回调不得重复生成调休批次"
        );
    }

    /// ④ `comp_mode = 2` 通过 → 只置状态，不生成批次（P5 不做金额）。
    #[tokio::test]
    async fn approved_pay_mode_creates_no_grant() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 17);
        let request = seed_request(
            &txn,
            employee.id,
            work_date,
            COMP_MODE_PAY,
            REQUEST_STATUS_PENDING,
            150,
        )
        .await;

        on_instance_finished_in_tx(&txn, ACTOR_ID, request.id, true)
            .await
            .unwrap();

        let updated = overtime_repo::find_overtime_by_id(&txn, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status, REQUEST_STATUS_APPROVED,
            "计加班费的单据通过后同样置「已通过」"
        );
        assert!(
            grants_of(&txn, employee.id).await.is_empty(),
            "计加班费不得生成调休批次"
        );
    }

    /// ⑤ 驳回 → 置「已驳回」且不产生任何额度。
    #[tokio::test]
    async fn rejected_request_produces_no_grant() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 18);
        let request = seed_request(
            &txn,
            employee.id,
            work_date,
            COMP_MODE_TIME_OFF,
            REQUEST_STATUS_PENDING,
            150,
        )
        .await;

        on_instance_finished_in_tx(&txn, ACTOR_ID, request.id, false)
            .await
            .unwrap();

        let updated = overtime_repo::find_overtime_by_id(&txn, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status, REQUEST_STATUS_REJECTED,
            "驳回后加班单应置「已驳回」"
        );
        assert!(
            grants_of(&txn, employee.id).await.is_empty(),
            "驳回不得产生调休额度"
        );
    }

    /// 撤销路径：调用方先置「已撤销」，终态回调（approved = false）不得把它改写成「已驳回」。
    #[tokio::test]
    async fn canceled_request_keeps_canceled_status_on_callback() {
        let txn = test_txn().await;
        let employee = seed_employee(&txn).await;
        let work_date = date(2097, 3, 19);
        let request = seed_request(
            &txn,
            employee.id,
            work_date,
            COMP_MODE_TIME_OFF,
            REQUEST_STATUS_CANCELED,
            150,
        )
        .await;

        on_instance_finished_in_tx(&txn, ACTOR_ID, request.id, false)
            .await
            .unwrap();

        let updated = overtime_repo::find_overtime_by_id(&txn, request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status, REQUEST_STATUS_CANCELED,
            "已撤销的单据不得被终态回调改写为已驳回"
        );
        assert!(
            grants_of(&txn, employee.id).await.is_empty(),
            "撤销不得产生调休额度"
        );
    }

    /// 调休有效期按月进位，月末不足取当月最后一天（不是 +90 天）。
    #[test]
    fn expire_at_rolls_natural_months_and_clamps_to_month_end() {
        assert_eq!(add_months(date(2026, 9, 14), 3), date(2026, 12, 14));
        assert_eq!(
            add_months(date(2026, 11, 30), 3),
            date(2027, 2, 28),
            "月末不足应取当月最后一天"
        );
    }
}
