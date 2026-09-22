//! 审批域数据访问原语（只拼 SQL：过滤 / 排序 / 分页 / 审计盖章 / 软删标记）。
//!
//! 约定：
//! - `hr_approval_flow` 是业务主表（软删），逐查询 `.filter(DeletedAt.is_null())`；
//!   模板节点 / 实例 / 节点记录**没有 `deleted_at` 列**（节点硬删重排，实例与记录是审批历史）；
//! - 写原语一律 `*_in_tx`：只收 `&DatabaseTransaction`，不自行 begin/commit；
//! - 审计字段由本层盖章：create 写 `created_by` + `updated_by`，update 只刷 `updated_by`；
//! - 「读 → 判断 → 写」才加行锁（`find_*_for_update`），列表 / 详情不加锁；
//! - 分页唯一执行器 `crate::utils::paginate`，分页查询必须带确定 `ORDER BY`；
//! - **节点解析所需的平台 / 员工只读查询也在这里**（`sys_user_dept` / `sys_user_role` /
//!   `sys_role` / `hr_employee` 实体是全局共享的，跨域调用他域 repo / service 才是禁止的），
//!   业务判断（谁能审、能否跳过）留在 service。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseTransaction, QueryOrder, QuerySelect};

use crate::entity::{
    hr_approval_flow, hr_approval_flow_node, hr_approval_instance, hr_approval_record, hr_employee,
    sys_role, sys_user_dept, sys_user_role,
};
use crate::modules::biz::hr::approval::dto::{
    FlowFilter, FlowNodeFilter, InstanceFilter, RecordFilter,
};
use crate::utils::PageData;

// —— 模板 ——

/// 分页 + 动态过滤审批流模板（keyword 模糊业务类型 / 模板名称，status 精确），
/// 按 id 降序，恒排除软删。
pub async fn find_flow_page(
    db: &impl ConnectionTrait,
    filter: &FlowFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_approval_flow::Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = &filter.keyword {
        let like_keyword = format!("%{keyword}%");
        cond = cond.add(
            Condition::any()
                .add(hr_approval_flow::Column::BizType.like(like_keyword.clone()))
                .add(hr_approval_flow::Column::Name.like(like_keyword)),
        );
    }
    if let Some(status) = filter.status {
        cond = cond.add(hr_approval_flow::Column::Status.eq(status));
    }

    let select = hr_approval_flow::Entity::find()
        .filter(cond)
        .filter(hr_approval_flow::Column::DeletedAt.is_null())
        .order_by_desc(hr_approval_flow::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查有效模板（排除软删）。
pub async fn find_flow_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_approval_flow::Model>> {
    Ok(hr_approval_flow::Entity::find()
        .filter(hr_approval_flow::Column::Id.eq(id))
        .filter(hr_approval_flow::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按 `biz_type` 查模板——**含软删占位**（不过滤 `deleted_at`）。
///
/// `uk_hr_approval_flow_biz_type` 是单列唯一键，软删行仍占位，查重必须能看到软删记录。
pub async fn find_flow_by_biz_type_include_deleted(
    db: &impl ConnectionTrait,
    biz_type: &str,
) -> anyhow::Result<Option<hr_approval_flow::Model>> {
    Ok(hr_approval_flow::Entity::find()
        .filter(hr_approval_flow::Column::BizType.eq(biz_type))
        .one(db)
        .await?)
}

/// 按 `biz_type` 查**启用**模板（审批启动用：软删 / 停用都视为没有模板）。
pub async fn find_enabled_flow_by_biz_type(
    db: &impl ConnectionTrait,
    biz_type: &str,
) -> anyhow::Result<Option<hr_approval_flow::Model>> {
    Ok(hr_approval_flow::Entity::find()
        .filter(hr_approval_flow::Column::BizType.eq(biz_type))
        .filter(hr_approval_flow::Column::Status.eq(super::FLOW_STATUS_ENABLED))
        .filter(hr_approval_flow::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 事务内创建模板：审计盖章（创建人与更新人同源）。
pub async fn create_flow_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_flow::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_approval_flow::Model> {
    Ok(hr_approval_flow::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    }
    .insert(txn)
    .await?)
}

/// 事务内更新模板（窄写）：只刷新更新人。
pub async fn update_flow_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_flow::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_approval_flow::Model> {
    Ok(hr_approval_flow::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    }
    .update(txn)
    .await?)
}

/// 事务内软删模板：返回是否命中（`false` = 已被并发删掉）。
pub async fn soft_delete_flow_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = hr_approval_flow::Entity::update_many()
        .set(hr_approval_flow::ActiveModel {
            deleted_at: Set(Some(chrono::Local::now().naive_local())),
            updated_by: Set(actor_id),
            ..Default::default()
        })
        .filter(hr_approval_flow::Column::Id.eq(id))
        .filter(hr_approval_flow::Column::DeletedAt.is_null())
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

// —— 模板节点 ——

/// 某模板的全部节点（按 `seq` 升序）。
pub async fn find_nodes_by_flow_id(
    db: &impl ConnectionTrait,
    flow_id: u64,
) -> anyhow::Result<Vec<hr_approval_flow_node::Model>> {
    Ok(hr_approval_flow_node::Entity::find()
        .filter(hr_approval_flow_node::Column::FlowId.eq(flow_id))
        .order_by_asc(hr_approval_flow_node::Column::Seq)
        .all(db)
        .await?)
}

/// 分页某模板的节点（按 `seq` 升序）。
pub async fn find_node_page(
    db: &impl ConnectionTrait,
    filter: &FlowNodeFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_approval_flow_node::Model>> {
    let select = hr_approval_flow_node::Entity::find()
        .filter(hr_approval_flow_node::Column::FlowId.eq(filter.flow_id))
        .order_by_asc(hr_approval_flow_node::Column::Seq);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查节点。
pub async fn find_node_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_approval_flow_node::Model>> {
    Ok(hr_approval_flow_node::Entity::find()
        .filter(hr_approval_flow_node::Column::Id.eq(id))
        .one(db)
        .await?)
}

/// 按 `(flow_id, seq)` 查节点（唯一键口径）。
pub async fn find_node_by_flow_seq(
    db: &impl ConnectionTrait,
    flow_id: u64,
    seq: i32,
) -> anyhow::Result<Option<hr_approval_flow_node::Model>> {
    Ok(hr_approval_flow_node::Entity::find()
        .filter(hr_approval_flow_node::Column::FlowId.eq(flow_id))
        .filter(hr_approval_flow_node::Column::Seq.eq(seq))
        .one(db)
        .await?)
}

/// 某模板 `seq` 最大的节点（= 最后一个节点，用于「最后一个节点不允许跳过」规则）。
pub async fn find_last_node_by_flow(
    db: &impl ConnectionTrait,
    flow_id: u64,
) -> anyhow::Result<Option<hr_approval_flow_node::Model>> {
    Ok(hr_approval_flow_node::Entity::find()
        .filter(hr_approval_flow_node::Column::FlowId.eq(flow_id))
        .order_by_desc(hr_approval_flow_node::Column::Seq)
        .one(db)
        .await?)
}

/// 某模板的节点数。
pub async fn count_nodes_by_flow(db: &impl ConnectionTrait, flow_id: u64) -> anyhow::Result<u64> {
    use sea_orm::PaginatorTrait;
    Ok(hr_approval_flow_node::Entity::find()
        .filter(hr_approval_flow_node::Column::FlowId.eq(flow_id))
        .count(db)
        .await?)
}

/// 事务内创建节点。
pub async fn create_node_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_flow_node::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_approval_flow_node::Model> {
    Ok(hr_approval_flow_node::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    }
    .insert(txn)
    .await?)
}

/// 事务内更新节点（窄写）。
pub async fn update_node_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_flow_node::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_approval_flow_node::Model> {
    Ok(hr_approval_flow_node::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    }
    .update(txn)
    .await?)
}

/// 事务内硬删节点（模板节点无软删列：删除后同 `seq` 可重排）。
pub async fn delete_node_in_tx(txn: &DatabaseTransaction, id: u64) -> anyhow::Result<bool> {
    let result = hr_approval_flow_node::Entity::delete_many()
        .filter(hr_approval_flow_node::Column::Id.eq(id))
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

// —— 实例 ——

/// 分页 + 动态过滤审批实例，按 id 降序。
pub async fn find_instance_page(
    db: &impl ConnectionTrait,
    filter: &InstanceFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_approval_instance::Model>> {
    let mut cond = Condition::all();

    if let Some(biz_type) = &filter.biz_type {
        cond = cond.add(hr_approval_instance::Column::BizType.eq(biz_type.clone()));
    }
    if let Some(status) = filter.status {
        cond = cond.add(hr_approval_instance::Column::Status.eq(status));
    }
    if let Some(applicant_id) = filter.applicant_id {
        cond = cond.add(hr_approval_instance::Column::ApplicantId.eq(applicant_id));
    }

    let select = hr_approval_instance::Entity::find()
        .filter(cond)
        .order_by_desc(hr_approval_instance::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 待办分页：审批中 + 当前节点轮到我（`current_approver_id` = 我）或我的角色池
/// （`current_approver_role_id ∈ 我的角色`）。
///
/// 单表查询：`current_approver_*` 是实例上的**当前节点快照**，随节点推进在同一事务内更新，
/// 因此不需要 join 节点记录表，也不受模板后续改动影响。
pub async fn find_todo_page(
    db: &impl ConnectionTrait,
    approver_id: u64,
    role_ids: &[u64],
    biz_type: Option<&str>,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_approval_instance::Model>> {
    let mut mine =
        Condition::any().add(hr_approval_instance::Column::CurrentApproverId.eq(approver_id));
    if !role_ids.is_empty() {
        mine = mine.add(
            hr_approval_instance::Column::CurrentApproverRoleId.is_in(role_ids.iter().copied()),
        );
    }

    let mut cond = Condition::all()
        .add(hr_approval_instance::Column::Status.eq(super::INSTANCE_STATUS_PENDING))
        .add(mine);
    if let Some(biz_type) = biz_type {
        cond = cond.add(hr_approval_instance::Column::BizType.eq(biz_type));
    }

    let select = hr_approval_instance::Entity::find()
        .filter(cond)
        .order_by_desc(hr_approval_instance::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 id 查实例（普通读，不加锁）。
pub async fn find_instance_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_approval_instance::Model>> {
    Ok(hr_approval_instance::Entity::find()
        .filter(hr_approval_instance::Column::Id.eq(id))
        .one(db)
        .await?)
}

/// 按 id 加锁读实例（`SELECT ... FOR UPDATE`）：审批动作前必须走它，串行化同一实例的并发审批。
pub async fn find_instance_by_id_for_update(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<Option<hr_approval_instance::Model>> {
    Ok(hr_approval_instance::Entity::find()
        .filter(hr_approval_instance::Column::Id.eq(id))
        .lock_exclusive()
        .one(txn)
        .await?)
}

/// 按业务类型 + 单据 ID 查**最新**实例。
///
/// 一张单据可以有**多条**历史实例：驳回 / 撤销后重新提交会新建一条（旧实例保留为审批历史），
/// 故这里必须按 id 降序取最新一条，否则重提交后的推进与展示会读到已终结的旧实例。
pub async fn find_instance_by_biz(
    db: &impl ConnectionTrait,
    biz_type: &str,
    biz_id: u64,
) -> anyhow::Result<Option<hr_approval_instance::Model>> {
    Ok(hr_approval_instance::Entity::find()
        .filter(hr_approval_instance::Column::BizType.eq(biz_type))
        .filter(hr_approval_instance::Column::BizId.eq(biz_id))
        .order_by_desc(hr_approval_instance::Column::Id)
        .one(db)
        .await?)
}

/// 某模板下审批中的实例数（模板删除护栏用）。
pub async fn count_pending_instances_by_flow(
    db: &impl ConnectionTrait,
    flow_id: u64,
) -> anyhow::Result<u64> {
    use sea_orm::PaginatorTrait;
    Ok(hr_approval_instance::Entity::find()
        .filter(hr_approval_instance::Column::FlowId.eq(flow_id))
        .filter(hr_approval_instance::Column::Status.eq(super::INSTANCE_STATUS_PENDING))
        .count(db)
        .await?)
}

/// 事务内创建实例。
pub async fn create_instance_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_instance::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_approval_instance::Model> {
    Ok(hr_approval_instance::ActiveModel {
        created_by: Set(actor_id),
        updated_by: Set(actor_id),
        ..model
    }
    .insert(txn)
    .await?)
}

/// 事务内更新实例（窄写）。
pub async fn update_instance_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_instance::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<hr_approval_instance::Model> {
    Ok(hr_approval_instance::ActiveModel {
        updated_by: Set(actor_id),
        ..model
    }
    .update(txn)
    .await?)
}

// —— 节点记录 ——

/// 某实例的全部节点记录（按 `seq` 升序）。
pub async fn find_records_by_instance_id(
    db: &impl ConnectionTrait,
    instance_id: u64,
) -> anyhow::Result<Vec<hr_approval_record::Model>> {
    Ok(hr_approval_record::Entity::find()
        .filter(hr_approval_record::Column::InstanceId.eq(instance_id))
        .order_by_asc(hr_approval_record::Column::Seq)
        .all(db)
        .await?)
}

/// 分页某实例的节点记录（按 `seq` 升序）。
pub async fn find_record_page(
    db: &impl ConnectionTrait,
    filter: &RecordFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<hr_approval_record::Model>> {
    let select = hr_approval_record::Entity::find()
        .filter(hr_approval_record::Column::InstanceId.eq(filter.instance_id))
        .order_by_asc(hr_approval_record::Column::Seq);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 按 `(instance_id, seq)` 查节点记录（唯一键口径）。
pub async fn find_record_by_instance_seq(
    db: &impl ConnectionTrait,
    instance_id: u64,
    seq: i32,
) -> anyhow::Result<Option<hr_approval_record::Model>> {
    Ok(hr_approval_record::Entity::find()
        .filter(hr_approval_record::Column::InstanceId.eq(instance_id))
        .filter(hr_approval_record::Column::Seq.eq(seq))
        .one(db)
        .await?)
}

/// 事务内创建节点记录（该表只有 `created_at` / `updated_at`，无审计列）。
pub async fn create_record_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_record::ActiveModel,
) -> anyhow::Result<hr_approval_record::Model> {
    Ok(model.insert(txn).await?)
}

/// 事务内更新节点记录（窄写）。
pub async fn update_record_in_tx(
    txn: &DatabaseTransaction,
    model: hr_approval_record::ActiveModel,
) -> anyhow::Result<hr_approval_record::Model> {
    Ok(model.update(txn).await?)
}

// —— 节点解析所需的只读查询（平台 / 员工实体是全局共享的）——

/// 按 `user_id` 查员工档案（软删视为不存在）。
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

/// 按员工 ID 查员工档案（软删视为不存在）——「直属上级」节点解析用。
pub async fn find_employee_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<hr_employee::Model>> {
    Ok(hr_employee::Entity::find()
        .filter(hr_employee::Column::Id.eq(id))
        .filter(hr_employee::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 某用户的**主部门** ID（`sys_user_dept.is_primary = 1`）。
pub async fn find_primary_dept_id_by_user(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Option<u64>> {
    Ok(sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::UserId.eq(user_id))
        .filter(sys_user_dept::Column::IsPrimary.eq(1))
        .one(db)
        .await?
        .map(|m| m.dept_id))
}

/// 某部门的负责人：`is_leader = 1` 的用户中取 `user_id` 最小者（多个负责人时的确定口径）。
pub async fn find_leader_user_id_by_dept(
    db: &impl ConnectionTrait,
    dept_id: u64,
) -> anyhow::Result<Option<u64>> {
    Ok(sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::DeptId.eq(dept_id))
        .filter(sys_user_dept::Column::IsLeader.eq(1))
        .order_by_asc(sys_user_dept::Column::UserId)
        .one(db)
        .await?
        .map(|m| m.user_id))
}

/// 某用户持有的**启用**角色 ID 列表（角色池待办与审批资格判定用）。
///
/// 角色停用 / 软删后其成员资格立即失效：与 `resolve_approver` 的 `find_enabled_role_by_id`
/// 同口径（否则停用角色的残留成员仍能进待办并审批，角色治理形同虚设）。
pub async fn find_role_ids_by_user_id(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Vec<u64>> {
    let role_ids: Vec<u64> = sys_user_role::Entity::find()
        .filter(sys_user_role::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .map(|m| m.role_id)
        .collect();
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(sys_role::Entity::find()
        .filter(sys_role::Column::Id.is_in(role_ids))
        .filter(sys_role::Column::Status.eq(1))
        .filter(sys_role::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect())
}

/// 角色池是否至少有一名**启用**成员（`node_type = 4` 的「有人能审」判定用）。
///
/// 角色存在且启用还不够：池子里没人 = 节点判「已解析」却无人可审，单据会卡在审批中。
pub async fn role_has_enabled_member(
    db: &impl ConnectionTrait,
    role_id: u64,
) -> anyhow::Result<bool> {
    use crate::entity::sys_user;
    use sea_orm::PaginatorTrait;

    let member_ids: Vec<u64> = sys_user_role::Entity::find()
        .filter(sys_user_role::Column::RoleId.eq(role_id))
        .all(db)
        .await?
        .into_iter()
        .map(|m| m.user_id)
        .collect();
    if member_ids.is_empty() {
        return Ok(false);
    }

    Ok(sys_user::Entity::find()
        .filter(sys_user::Column::Id.is_in(member_ids))
        .filter(sys_user::Column::Status.eq(1))
        .filter(sys_user::Column::DeletedAt.is_null())
        .count(db)
        .await?
        > 0)
}

/// 按 id 查启用角色（软删 / 停用视为不存在）——`node_type = 4` 的引用校验用。
pub async fn find_enabled_role_by_id(
    db: &impl ConnectionTrait,
    role_id: u64,
) -> anyhow::Result<Option<sys_role::Model>> {
    Ok(sys_role::Entity::find()
        .filter(sys_role::Column::Id.eq(role_id))
        .filter(sys_role::Column::Status.eq(1))
        .filter(sys_role::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}

/// 按 id 查启用用户（`sys_user.status = 1` 且未软删）——`node_type = 3` 的引用校验用。
pub async fn find_enabled_user_by_id(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> anyhow::Result<Option<crate::entity::sys_user::Model>> {
    use crate::entity::sys_user;
    Ok(sys_user::Entity::find()
        .filter(sys_user::Column::Id.eq(user_id))
        .filter(sys_user::Column::Status.eq(1))
        .filter(sys_user::Column::DeletedAt.is_null())
        .one(db)
        .await?)
}
