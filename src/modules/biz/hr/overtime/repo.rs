//! 加班域数据访问原语（只拼 SQL：过滤 / 排序 / 分页 / 审计盖章 / 软删标记）。
//!
//! 约定：
//! - `hr_overtime_request` 是业务主表（软删），逐查询 `.filter(DeletedAt.is_null())`；
//! - 写原语一律 `*_in_tx`：只收 `&DatabaseTransaction`，不自行 begin/commit；
//! - 审计字段由本层盖章：create 写 `created_by` + `updated_by`，update 只刷 `updated_by`；
//! - 「读 → 判断 → 写」才加行锁（`find_overtime_by_id_for_update` / `lock_employee_for_update`），
//!   列表 / 详情不加锁；
//! - **区间重叠是「读 → 判断 → 写」**：先 [`lock_employee_for_update`] 锁住员工行把同一员工的
//!   并发建单串行化，再查重叠——否则两个并发请求会同时看到「无重叠」而各写一条；
//! - 分页唯一执行器 `crate::utils::paginate`，分页查询必须带确定 `ORDER BY`；
//! - 员工档案 / 假期类型的只读查询也在这里（`hr_employee` / `hr_time_off_type` 是全局共享实体，
//!   跨域调用他域 repo / service 才是禁止的）；业务判断（能否建单、算不算加班）留在 service。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseTransaction, QueryOrder, QuerySelect};

use crate::entity::{hr_employee, hr_overtime_request, hr_time_off_type};
use crate::modules::biz::hr::overtime::dto::OvertimeFilter;
use crate::utils::PageData;

// —— 加班单 ——

/// 分页 + 动态过滤加班单（employee_id / status / overtime_type 精确，`work_date` 区间），
/// 按 id 降序（新单在前），恒排除软删。
pub async fn find_overtime_page(
    db: &impl ConnectionTrait,
    filter: &OvertimeFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_overtime_request::Model>> {
    let mut cond = Condition::all();

    if let Some(employee_id) = filter.employee_id {
        cond = cond.add(hr_overtime_request::Column::EmployeeId.eq(employee_id));
    }
    if let Some(status) = filter.status {
        cond = cond.add(hr_overtime_request::Column::Status.eq(status));
    }
    if let Some(overtime_type) = filter.overtime_type {
        cond = cond.add(hr_overtime_request::Column::OvertimeType.eq(overtime_type));
    }
    if let Some(begin) = filter.work_date_begin {
        cond = cond.add(hr_overtime_request::Column::WorkDate.gte(begin));
    }
    if let Some(end) = filter.work_date_end {
        cond = cond.add(hr_overtime_request::Column::WorkDate.lte(end));
    }

    let select = hr_overtime_request::Entity::find()
        .filter(cond)
        .filter(hr_overtime_request::Column::DeletedAt.is_null())
        .order_by_desc(hr_overtime_request::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查加班单（软删视为不存在）。
pub async fn find_overtime_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_overtime_request::Model>> {
    Ok(hr_overtime_request::Entity::find()
        .filter(hr_overtime_request::Column::Id.eq(id))
        .filter(hr_overtime_request::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按 id 加锁读加班单（`SELECT ... FOR UPDATE`）：提交 / 撤销 / 删除 / 审批终态回调
/// 这些「读 → 判断 → 写」路径必须走它（并发重复回调不会各写一遍）。
pub async fn find_overtime_by_id_for_update(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<Option<hr_overtime_request::Model>> {
    Ok(hr_overtime_request::Entity::find()
        .filter(hr_overtime_request::Column::Id.eq(id))
        .filter(hr_overtime_request::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(txn)
        .await?)
}

/// 查同员工、同 `work_date`、**已通过**的加班单里与给定区间重叠的行。
///
/// 重叠判据：`start_at < end_at_new AND end_at > start_at_new`（左闭右开语义下的区间相交）。
/// `work_date` 已在 service 层要求与起止时间同日，故跨天单据不会漏（本域不接受跨天提交）。
pub async fn find_overlapping_approved_overtime(
    db: &impl ConnectionTrait,
    employee_id: u64,
    work_date: Date,
    start_at: DateTime,
    end_at: DateTime,
) -> anyhow::Result<Vec<hr_overtime_request::Model>> {
    let cond = Condition::all()
        .add(hr_overtime_request::Column::EmployeeId.eq(employee_id))
        .add(hr_overtime_request::Column::WorkDate.eq(work_date))
        .add(hr_overtime_request::Column::Status.eq(super::REQUEST_STATUS_APPROVED))
        .add(hr_overtime_request::Column::StartAt.lt(end_at))
        .add(hr_overtime_request::Column::EndAt.gt(start_at));

    Ok(hr_overtime_request::Entity::find()
        .filter(cond)
        .filter(hr_overtime_request::Column::DeletedAt.is_null())
        .order_by_asc(hr_overtime_request::Column::Id)
        .all(db)
        .await?)
}

/// 事务内创建加班单：审计盖章（创建人与更新人同源）。
pub async fn create_overtime_in_tx(
    txn: &DatabaseTransaction,
    model: hr_overtime_request::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_overtime_request::Model> {
    Ok(hr_overtime_request::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    }
    .insert(txn)
    .await?)
}

/// 事务内更新加班单（窄写）：只刷新更新人，`created_by` 保持 `NotSet` 不被覆盖。
///
/// 入参 `model` 由 service 构造：只 Set 业务变更列，其余列留 `NotSet`。
pub async fn update_overtime_in_tx(
    txn: &DatabaseTransaction,
    model: hr_overtime_request::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_overtime_request::Model> {
    Ok(hr_overtime_request::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    }
    .update(txn)
    .await?)
}

/// 事务内软删加班单：只更新 `deleted_at` + `updated_by`。
///
/// 返回是否有行被更新（`false` = 目标不存在或已软删）；「单据不存在」的文案由 service 负责。
pub async fn soft_delete_overtime_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = hr_overtime_request::Entity::update_many()
        .set(hr_overtime_request::ActiveModel {
            deleted_at: Set(Some(chrono::Local::now().naive_local())),
            updated_by: Set(actor_id),
            ..Default::default()
        })
        .filter(hr_overtime_request::Column::Id.eq(id))
        .filter(hr_overtime_request::Column::DeletedAt.is_null())
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

// —— 员工档案（全局共享实体的只读 / 加锁查询）——

/// 加锁读员工档案（`SELECT ... FOR UPDATE`，软删视为不存在）。
///
/// 加班单的「同日区间不重叠」是「读 → 判断 → 写」：先锁员工行，把同一员工的并发建单串行化。
/// 锁的是**员工行**而不是加班单行（后者尚不存在），锁粒度按员工自然正确。
pub async fn lock_employee_for_update(
    txn: &DatabaseTransaction,
    employee_id: u64,
) -> anyhow::Result<Option<hr_employee::Model>> {
    Ok(hr_employee::Entity::find()
        .filter(hr_employee::Column::Id.eq(employee_id))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(txn)
        .await?)
}

/// 按 id 查员工档案（软删视为不存在）。
pub async fn find_employee_by_id(
    db: &impl ConnectionTrait,
    employee_id: u64,
) -> anyhow::Result<Option<hr_employee::Model>> {
    Ok(hr_employee::Entity::find()
        .filter(hr_employee::Column::Id.eq(employee_id))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按平台用户 ID 查员工档案（软删视为不存在）——「我的单据」按 `user_id` 反查档案用。
pub async fn find_employee_by_user_id(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Option<hr_employee::Model>> {
    Ok(hr_employee::Entity::find()
        .filter(hr_employee::Column::UserId.eq(user_id))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按员工 ID 批量取档案（列表响应的员工名拼装用；空入参早返，不发 `IN ()`）。
pub async fn find_employees_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> anyhow::Result<Vec<hr_employee::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(hr_employee::Entity::find()
        .filter(hr_employee::Column::Id.is_in(ids.iter().copied()))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .all(db)
        .await?)
}

// —— 假期类型（加班转调休的入账落点）——

/// 按 `type_code` 查**启用**的假期类型（软删 / 停用视为不存在）。
///
/// 加班转调休要落进「调休」假别（种子 `type_code = comp`），ID 由查库得到，**不写死数字**。
pub async fn find_enabled_time_off_type_by_code(
    db: &impl ConnectionTrait,
    type_code: &str,
) -> anyhow::Result<Option<hr_time_off_type::Model>> {
    Ok(hr_time_off_type::Entity::find()
        .filter(hr_time_off_type::Column::TypeCode.eq(type_code))
        .filter(hr_time_off_type::Column::Status.eq(super::super::time_off::TIME_OFF_TYPE_ENABLED))
        .filter(hr_time_off_type::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}
