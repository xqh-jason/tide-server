//! 审批域业务：模板 / 模板节点 CRUD + 实例推进（多节点顺序）+ 终态分派。
//!
//! 分层约定（见 AGENTS.md「分层契约」）：
//! - 自持事务的入口收 `db: &DatabaseConnection`，内部委托 `pub(crate) *_in_tx(txn, …)`；
//! - 只读入口收 `&impl ConnectionTrait`，不起事务；
//! - `repo` 只拼 SQL；「谁能审」「能不能跳过」「业务类型是否已接入」这类判断在本层；
//!   值域类校验（status / biz_type 是否在字典里）在 `validate.rs`，由 api 层预取字典后传入。
//!
//! # 推进语义（roadmap §3.5）
//!
//! - 提交时**一次展开**全部节点记录：解析得到的审批人记 `action = 0 待审批`，
//!   解析不到且该节点 `skip_if_empty = 1` 直接记 `action = 3 跳过`；
//!   **最后一个节点恒不允许跳过**（`upsert_flow_node` / `delete_flow_node` 双重兜底），
//!   解析不到就报错让申请人找 HR，避免单据无人把关；
//! - 通过：当前 `seq` 节点置通过 → 找下一个 `action = 0` 的节点作为当前节点；
//!   没有下一个则实例 `status = 2 已通过`；
//! - 驳回：实例 `status = 3 已驳回`（后续节点不激活）；撤销：`status = 4 已撤销`；
//! - 并发：审批动作前 `find_instance_by_id_for_update`（`SELECT ... FOR UPDATE`）串行化
//!   同一实例的两个审批人，并复核 `record.action == 0`（角色池两人同时点「通过」只生效一次）；
//! - **终态分派**：本层按 `biz_type` match 到业务域 `on_instance_finished_in_tx`
//!   （`hr/time-off` / `hr/overtime`），额度实扣 / 释放 / 调休入账都在**同一事务**里完成。

use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::{
    hr_approval_flow, hr_approval_flow_node, hr_approval_instance, hr_approval_record,
};
use crate::modules::biz::hr::approval::dto::{
    CreateFlowReq, FlowFilter, FlowListReq, FlowNodeFilter, FlowNodeListReq, InstanceFilter,
    InstanceListReq, RecordFilter, RecordListReq, TodoListReq, UpdateFlowReq, UpsertFlowNodeReq,
};
use crate::modules::biz::hr::approval::{
    ACTION_APPROVED, ACTION_PENDING, ACTION_REJECTED, ACTION_SKIPPED, BIZ_TYPE_OVERTIME,
    BIZ_TYPE_TIME_OFF, CURRENT_SEQ_NONE, INSTANCE_STATUS_APPROVED, INSTANCE_STATUS_CANCELED,
    INSTANCE_STATUS_PENDING, INSTANCE_STATUS_REJECTED, MAX_FLOW_NODES, NODE_TYPE_ROLE,
    repo as approval_repo,
};
use crate::utils::PageData;
use crate::utils::error::AppError;
use sea_orm::ActiveValue::Set;

/// 在职状态「离职」（字典 `employmentStatus` 的 3）——离职的上级不再作为审批人。
const EMPLOYMENT_STATUS_RESIGNED: i8 = 3;

/// 当前时间（与仓储层的 `created_at` 默认值同口径：本地时间落 `DATETIME`）。
fn now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

// —— 模板 ——

/// 模板分页（纯透传过滤条件）。
pub async fn page_flows(
    db: &impl ConnectionTrait,
    req: &FlowListReq,
) -> Result<PageData<hr_approval_flow::Model>, AppError> {
    let filter = FlowFilter {
        keyword: req.keyword.clone(),
        status: req.status,
    };
    Ok(
        approval_repo::find_flow_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 事务内创建模板：`biz_type` 单列唯一（含软删占位）→ 查重后落库。
pub(crate) async fn create_flow_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateFlowReq,
) -> Result<hr_approval_flow::Model, AppError> {
    let biz_type = req.biz_type.trim();
    if approval_repo::find_flow_by_biz_type_include_deleted(txn, biz_type)
        .await?
        .is_some()
    {
        return Err(AppError::Biz(format!("该业务类型已存在审批流：{biz_type}")));
    }

    Ok(approval_repo::create_flow_in_tx(
        txn,
        hr_approval_flow::ActiveModel {
            biz_type: Set(biz_type.to_string()),
            name: Set(req.name.trim().to_string()),
            status: Set(req.status),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 事务内更新模板：判存在 → `biz_type` 变更时查重（排除自身）→ 窄写。
pub(crate) async fn update_flow_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateFlowReq,
) -> Result<hr_approval_flow::Model, AppError> {
    approval_repo::find_flow_by_id(txn, req.id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批流不存在：{}", req.id)))?;

    let biz_type = req.biz_type.trim();
    if let Some(existing) =
        approval_repo::find_flow_by_biz_type_include_deleted(txn, biz_type).await?
        && existing.id != req.id
    {
        return Err(AppError::Biz(format!("该业务类型已存在审批流：{biz_type}")));
    }

    Ok(approval_repo::update_flow_in_tx(
        txn,
        hr_approval_flow::ActiveModel {
            id: Set(req.id),
            biz_type: Set(biz_type.to_string()),
            name: Set(req.name.trim().to_string()),
            status: Set(req.status),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 事务内软删模板：有审批中的实例时拒绝（在途单据会失去推进依据）。
pub(crate) async fn delete_flow_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    approval_repo::find_flow_by_id(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批流不存在：{id}")))?;

    let pending = approval_repo::count_pending_instances_by_flow(txn, id).await?;
    if pending > 0 {
        return Err(AppError::Biz(format!(
            "该审批流还有 {pending} 条审批中的单据，不能删除"
        )));
    }

    if !approval_repo::soft_delete_flow_in_tx(txn, id, actor_id).await? {
        return Err(AppError::Biz(format!("审批流不存在：{id}")));
    }
    Ok(())
}

/// 模板详情（模板 + 节点，按 `seq` 升序）。
pub async fn get_flow(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<(hr_approval_flow::Model, Vec<hr_approval_flow_node::Model>), AppError> {
    let flow = approval_repo::find_flow_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批流不存在：{id}")))?;
    let nodes = approval_repo::find_nodes_by_flow_id(db, id).await?;
    Ok((flow, nodes))
}

// —— 模板节点 ——

/// 节点分页。
pub async fn page_flow_nodes(
    db: &impl ConnectionTrait,
    req: &FlowNodeListReq,
) -> Result<PageData<hr_approval_flow_node::Model>, AppError> {
    let filter = FlowNodeFilter {
        flow_id: req.flow_id,
    };
    Ok(
        approval_repo::find_node_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 事务内新增 / 修改节点（`id = 0` 新增）。
///
/// 规则（都需要查库，故在 service）：
/// - 所属模板必须存在；
/// - `node_type = 3 指定用户` / `4 指定角色` 的引用必须存在且启用（否则该节点永远解析不到人）；
/// - `(flow_id, seq)` 唯一（新增占用 / 改成已占用的 seq 都拒绝）；
/// - **最后一个节点不允许跳过**：写入后复核最后一个节点，`skip_if_empty = 1` 直接失败回滚。
pub(crate) async fn upsert_flow_node_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpsertFlowNodeReq,
) -> Result<hr_approval_flow_node::Model, AppError> {
    approval_repo::find_flow_by_id(txn, req.flow_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批流不存在：{}", req.flow_id)))?;

    if req.node_type == crate::modules::biz::hr::approval::NODE_TYPE_USER
        && approval_repo::find_enabled_user_by_id(txn, req.approver_ref_id)
            .await?
            .is_none()
    {
        return Err(AppError::Biz(format!(
            "指定的审批用户不存在或已停用：{}",
            req.approver_ref_id
        )));
    }
    if req.node_type == NODE_TYPE_ROLE
        && approval_repo::find_enabled_role_by_id(txn, req.approver_ref_id)
            .await?
            .is_none()
    {
        return Err(AppError::Biz(format!(
            "指定的审批角色不存在或已停用：{}",
            req.approver_ref_id
        )));
    }

    if let Some(occupied) = approval_repo::find_node_by_flow_seq(txn, req.flow_id, req.seq).await?
        && occupied.id != req.id
    {
        return Err(AppError::Biz(format!("该顺序号已被占用：{}", req.seq)));
    }

    let model = if req.id == 0 {
        let count = approval_repo::count_nodes_by_flow(txn, req.flow_id).await?;
        if count >= MAX_FLOW_NODES {
            return Err(AppError::Biz(format!(
                "审批节点数已达上限（{MAX_FLOW_NODES} 个）"
            )));
        }
        approval_repo::create_node_in_tx(
            txn,
            hr_approval_flow_node::ActiveModel {
                flow_id: Set(req.flow_id),
                seq: Set(req.seq),
                node_name: Set(req.node_name.trim().to_string()),
                node_type: Set(req.node_type),
                approver_ref_id: Set(req.approver_ref_id),
                skip_if_empty: Set(req.skip_if_empty),
                remark: Set(req.remark.clone()),
                ..Default::default()
            },
            actor_id,
        )
        .await?
    } else {
        let existing = approval_repo::find_node_by_id(txn, req.id)
            .await?
            .ok_or_else(|| AppError::Biz(format!("审批节点不存在：{}", req.id)))?;
        if existing.flow_id != req.flow_id {
            return Err(AppError::Biz(format!("审批节点不属于该审批流：{}", req.id)));
        }
        approval_repo::update_node_in_tx(
            txn,
            hr_approval_flow_node::ActiveModel {
                id: Set(req.id),
                seq: Set(req.seq),
                node_name: Set(req.node_name.trim().to_string()),
                node_type: Set(req.node_type),
                approver_ref_id: Set(req.approver_ref_id),
                skip_if_empty: Set(req.skip_if_empty),
                remark: Set(req.remark.clone()),
                ..Default::default()
            },
            actor_id,
        )
        .await?
    };

    ensure_last_node_not_skippable(txn, req.flow_id).await?;
    Ok(model)
}

/// 事务内硬删节点，并复核「最后一个节点不允许跳过」。
pub(crate) async fn delete_flow_node_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
) -> Result<(), AppError> {
    let node = approval_repo::find_node_by_id(txn, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批节点不存在：{id}")))?;
    if !approval_repo::delete_node_in_tx(txn, id).await? {
        return Err(AppError::Biz(format!("审批节点不存在：{id}")));
    }
    // 删掉最后一个节点后，新的最后一个节点可能配了「可跳过」——拒绝并让调用方先改配置
    ensure_last_node_not_skippable(txn, node.flow_id).await
}

/// 复核：模板的最后一个节点不允许「解析不到审批人就跳过」。
///
/// 为什么放在写之后：节点的「最后一个」身份由 `seq` 决定，新增 / 改 `seq` / 删除都可能
/// 让另一个节点变成最后一个；写入后统一复核一次，比在三个分支里各推演一遍可靠。
async fn ensure_last_node_not_skippable(
    txn: &DatabaseTransaction,
    flow_id: u64,
) -> Result<(), AppError> {
    let Some(last) = approval_repo::find_last_node_by_flow(txn, flow_id).await? else {
        // 模板暂无节点：允许（提交时会报「审批流未配置节点」）
        return Ok(());
    };
    if last.skip_if_empty == 1 {
        return Err(AppError::Biz(format!(
            "最后一个审批节点「{}」不允许跳过（解析不到审批人时必须报错），请先修改该节点的跳过设置",
            last.node_name
        )));
    }
    Ok(())
}

// —— 实例：启动与推进 ——

/// 启动审批（同事务调用方：`hr/time-off` 提交 / 重提交、`hr/overtime` 提交 / 重提交）：
/// 返回实例 ID。
///
/// 幂等规则（两个分支都不重复建实例）：
/// - 同一 `(biz_type, biz_id)` 已有**审批中**的实例 → 直接复用（重复提交不会开出第二条实例）；
/// - 已有实例但已终结（已通过 / 已驳回 / 已撤销）→ **另起一条新实例**。这是「驳回后修改并重新提交」
///   的必要条件：终态实例不可推进，若复用旧实例，单据会被置回审批中却永远推不动。
///   旧实例保留为审批历史（`find_instance_by_biz` 取最新一条）。
pub(crate) async fn start_instance_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    biz_type: &str,
    biz_id: u64,
    applicant_id: u64,
) -> Result<u64, AppError> {
    if let Some(existing) = approval_repo::find_instance_by_biz(txn, biz_type, biz_id).await?
        && existing.status == INSTANCE_STATUS_PENDING
    {
        return Ok(existing.id);
    }

    let flow = approval_repo::find_enabled_flow_by_biz_type(txn, biz_type)
        .await?
        .ok_or_else(|| AppError::Biz(format!("未配置启用的审批流：{biz_type}")))?;
    let nodes = approval_repo::find_nodes_by_flow_id(txn, flow.id).await?;
    if nodes.is_empty() {
        return Err(AppError::Biz(format!("审批流未配置节点：{}", flow.name)));
    }

    // 一次展开：解析每个节点的审批人，解析不到且允许跳过就记「跳过」
    let last_seq = nodes.last().map(|n| n.seq).unwrap_or(CURRENT_SEQ_NONE);
    let mut planned = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let (approver_id, approver_role_id) = resolve_approver(txn, applicant_id, node).await?;
        let resolved = approver_id > 0 || approver_role_id > 0;
        let action = if resolved {
            ACTION_PENDING
        } else if node.skip_if_empty == 1 && node.seq != last_seq {
            ACTION_SKIPPED
        } else {
            return Err(AppError::Biz(format!(
                "审批节点「{}」没有可用的审批人，请联系管理员配置审批流",
                node.node_name
            )));
        };
        planned.push((node, approver_id, approver_role_id, action));
    }

    let current = planned
        .iter()
        .find(|(_, _, _, action)| *action == ACTION_PENDING)
        .ok_or_else(|| AppError::Biz("审批流没有可用的审批节点，请检查审批流配置".into()))?;
    let (current_seq, current_approver_id, current_approver_role_id) =
        (current.0.seq, current.1, current.2);

    let instance = approval_repo::create_instance_in_tx(
        txn,
        hr_approval_instance::ActiveModel {
            biz_type: Set(biz_type.to_string()),
            biz_id: Set(biz_id),
            flow_id: Set(flow.id),
            applicant_id: Set(applicant_id),
            current_seq: Set(current_seq),
            current_approver_id: Set(current_approver_id),
            current_approver_role_id: Set(current_approver_role_id),
            status: Set(INSTANCE_STATUS_PENDING),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    for (node, approver_id, approver_role_id, action) in planned {
        approval_repo::create_record_in_tx(
            txn,
            hr_approval_record::ActiveModel {
                instance_id: Set(instance.id),
                seq: Set(node.seq),
                node_name: Set(node.node_name.clone()),
                node_type: Set(node.node_type),
                approver_id: Set(approver_id),
                approver_ref_id: Set(if approver_role_id > 0 {
                    approver_role_id
                } else {
                    node.approver_ref_id
                }),
                action: Set(action),
                ..Default::default()
            },
        )
        .await?;
    }

    Ok(instance.id)
}

/// 通过当前节点（审批人动作）：推进到下一节点，或把实例置为已通过并分派业务副作用。
pub(crate) async fn approve_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    instance_id: u64,
    opinion: &str,
) -> Result<(), AppError> {
    let (instance, record) = lock_pending_node(txn, instance_id).await?;
    ensure_can_act(txn, actor_id, &record).await?;

    approval_repo::update_record_in_tx(
        txn,
        hr_approval_record::ActiveModel {
            id: Set(record.id),
            action: Set(ACTION_APPROVED),
            opinion: Set(opinion.trim().to_string()),
            acted_at: Set(Some(now())),
            acted_by: Set(actor_id),
            ..Default::default()
        },
    )
    .await?;

    // 下一个待审批节点：提交时已展开，这里只按 seq 找第一个未处理的
    let records = approval_repo::find_records_by_instance_id(txn, instance_id).await?;
    match records
        .iter()
        .find(|r| r.seq > record.seq && r.action == ACTION_PENDING)
    {
        Some(next) => {
            let role_id = if next.node_type == NODE_TYPE_ROLE {
                next.approver_ref_id
            } else {
                0
            };
            approval_repo::update_instance_in_tx(
                txn,
                hr_approval_instance::ActiveModel {
                    id: Set(instance.id),
                    current_seq: Set(next.seq),
                    current_approver_id: Set(next.approver_id),
                    current_approver_role_id: Set(role_id),
                    ..Default::default()
                },
                actor_id,
            )
            .await?;
            Ok(())
        }
        None => {
            let finished =
                finish_instance_in_tx(txn, actor_id, instance.id, INSTANCE_STATUS_APPROVED).await?;
            dispatch_terminal_in_tx(txn, actor_id, &finished).await
        }
    }
}

/// 驳回当前节点：实例直接置已驳回（后续节点不激活），分派业务副作用（释放预占）。
pub(crate) async fn reject_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    instance_id: u64,
    opinion: &str,
) -> Result<(), AppError> {
    let (instance, record) = lock_pending_node(txn, instance_id).await?;
    ensure_can_act(txn, actor_id, &record).await?;

    approval_repo::update_record_in_tx(
        txn,
        hr_approval_record::ActiveModel {
            id: Set(record.id),
            action: Set(ACTION_REJECTED),
            opinion: Set(opinion.trim().to_string()),
            acted_at: Set(Some(now())),
            acted_by: Set(actor_id),
            ..Default::default()
        },
    )
    .await?;

    let finished =
        finish_instance_in_tx(txn, actor_id, instance.id, INSTANCE_STATUS_REJECTED).await?;
    dispatch_terminal_in_tx(txn, actor_id, &finished).await
}

/// 撤销实例（申请人动作 / 单据被删除）：未终结才可撤销，分派业务副作用（释放预占）。
pub(crate) async fn cancel_instance_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    instance_id: u64,
) -> Result<(), AppError> {
    let instance = approval_repo::find_instance_by_id_for_update(txn, instance_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批实例不存在：{instance_id}")))?;
    if instance.status != INSTANCE_STATUS_PENDING {
        return Err(AppError::Biz("该单据已完成审批，不能撤销".into()));
    }
    if instance.applicant_id != actor_id {
        return Err(AppError::Biz("只有申请人可以撤销该单据".into()));
    }

    let finished =
        finish_instance_in_tx(txn, actor_id, instance.id, INSTANCE_STATUS_CANCELED).await?;
    dispatch_terminal_in_tx(txn, actor_id, &finished).await
}

/// 按业务单据撤销在途实例（单据软删 / 撤回时由业务域调用）：无实例或已终结都是 no-op。
pub(crate) async fn cancel_by_biz_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    biz_type: &str,
    biz_id: u64,
) -> Result<(), AppError> {
    let Some(instance) = approval_repo::find_instance_by_biz(txn, biz_type, biz_id).await? else {
        return Ok(());
    };
    let instance = approval_repo::find_instance_by_id_for_update(txn, instance.id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批实例不存在：{}", instance.id)))?;
    if instance.status != INSTANCE_STATUS_PENDING {
        return Ok(());
    }

    let finished =
        finish_instance_in_tx(txn, actor_id, instance.id, INSTANCE_STATUS_CANCELED).await?;
    dispatch_terminal_in_tx(txn, actor_id, &finished).await
}

/// 加锁读实例 + 取当前节点记录，并校验实例仍在审批中。
async fn lock_pending_node(
    txn: &DatabaseTransaction,
    instance_id: u64,
) -> Result<(hr_approval_instance::Model, hr_approval_record::Model), AppError> {
    let instance = approval_repo::find_instance_by_id_for_update(txn, instance_id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批实例不存在：{instance_id}")))?;
    if instance.status != INSTANCE_STATUS_PENDING {
        return Err(AppError::Biz("该单据已完成审批，不能重复操作".into()));
    }

    let record = approval_repo::find_record_by_instance_seq(txn, instance_id, instance.current_seq)
        .await?
        .ok_or_else(|| AppError::Biz("审批节点记录缺失，请检查审批流配置".into()))?;
    if record.action != ACTION_PENDING {
        return Err(AppError::Biz("当前节点已被处理，请刷新后重试".into()));
    }
    Ok((instance, record))
}

/// 实例置终态：清空当前节点审批人 + 写终态时间。
async fn finish_instance_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    instance_id: u64,
    status: i8,
) -> Result<hr_approval_instance::Model, AppError> {
    Ok(approval_repo::update_instance_in_tx(
        txn,
        hr_approval_instance::ActiveModel {
            id: Set(instance_id),
            status: Set(status),
            current_seq: Set(CURRENT_SEQ_NONE),
            current_approver_id: Set(0),
            current_approver_role_id: Set(0),
            finished_at: Set(Some(now())),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 终态分派：按 `biz_type` 调业务域的同事务回调（额度实扣 / 释放 / 调休入账）。
///
/// 业务类型在 `create_flow` 时已按字典 `approvalBizType` + 本 match 双重收口，
/// 这里兜底报错（配置了没人处理的业务类型必须吵，而不是静默吞掉副作用）。
async fn dispatch_terminal_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    instance: &hr_approval_instance::Model,
) -> Result<(), AppError> {
    let approved = instance.status == INSTANCE_STATUS_APPROVED;
    match instance.biz_type.as_str() {
        BIZ_TYPE_TIME_OFF => {
            crate::modules::biz::hr::time_off::service::on_instance_finished_in_tx(
                txn,
                actor_id,
                instance.biz_id,
                approved,
            )
            .await
        }
        BIZ_TYPE_OVERTIME => {
            crate::modules::biz::hr::overtime::service::on_instance_finished_in_tx(
                txn,
                actor_id,
                instance.biz_id,
                approved,
            )
            .await
        }
        other => Err(AppError::Biz(format!("未知的业务类型：{other}"))),
    }
}

/// 校验操作人有权处理该节点：直接指定到人，或持有该节点角色池要求的角色。
async fn ensure_can_act(
    db: &impl ConnectionTrait,
    actor_id: u64,
    record: &hr_approval_record::Model,
) -> Result<(), AppError> {
    if record.approver_id == actor_id {
        return Ok(());
    }
    if record.node_type == NODE_TYPE_ROLE && record.approver_ref_id > 0 {
        let role_ids = approval_repo::find_role_ids_by_user_id(db, actor_id).await?;
        if role_ids.contains(&record.approver_ref_id) {
            return Ok(());
        }
    }
    Err(AppError::Biz("当前节点不由你审批".into()))
}

/// 解析节点审批人：返回 `(审批人 user_id, 角色池 role_id)`（两者其一为 0）。
///
/// 解析不到一律返回 `(0, 0)`，由调用方决定「跳过」还是报错——本函数不写业务决策。
async fn resolve_approver(
    db: &impl ConnectionTrait,
    applicant_id: u64,
    node: &hr_approval_flow_node::Model,
) -> Result<(u64, u64), AppError> {
    use crate::modules::biz::hr::approval::{
        NODE_TYPE_DEPT_LEADER, NODE_TYPE_MANAGER, NODE_TYPE_USER,
    };

    match node.node_type {
        NODE_TYPE_MANAGER => {
            let Some(applicant) = approval_repo::find_employee_by_user_id(db, applicant_id).await?
            else {
                return Ok((0, 0));
            };
            if applicant.manager_employee_id == 0 {
                return Ok((0, 0));
            }
            let Some(manager) =
                approval_repo::find_employee_by_id(db, applicant.manager_employee_id).await?
            else {
                return Ok((0, 0));
            };
            // 已离职的上级不再作为审批人（解析不到 → 跳过或报错）
            if manager.employment_status == EMPLOYMENT_STATUS_RESIGNED {
                return Ok((0, 0));
            }
            Ok((manager.user_id, 0))
        }
        NODE_TYPE_DEPT_LEADER => {
            let Some(dept_id) =
                approval_repo::find_primary_dept_id_by_user(db, applicant_id).await?
            else {
                return Ok((0, 0));
            };
            Ok((
                approval_repo::find_leader_user_id_by_dept(db, dept_id)
                    .await?
                    .unwrap_or(0),
                0,
            ))
        }
        NODE_TYPE_USER => {
            if approval_repo::find_enabled_user_by_id(db, node.approver_ref_id)
                .await?
                .is_some()
            {
                Ok((node.approver_ref_id, 0))
            } else {
                Ok((0, 0))
            }
        }
        NODE_TYPE_ROLE => {
            if approval_repo::find_enabled_role_by_id(db, node.approver_ref_id)
                .await?
                .is_some()
            {
                Ok((0, node.approver_ref_id))
            } else {
                Ok((0, 0))
            }
        }
        other => Err(AppError::Biz(format!("未知的审批节点类型：{other}"))),
    }
}

// —— 实例：查询 ——

/// 实例分页。
pub async fn page_instances(
    db: &impl ConnectionTrait,
    req: &InstanceListReq,
) -> Result<PageData<hr_approval_instance::Model>, AppError> {
    let filter = InstanceFilter {
        biz_type: req.biz_type.clone(),
        status: req.status,
        applicant_id: req.applicant_id,
    };
    Ok(
        approval_repo::find_instance_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 我的待办：审批中 + 当前节点轮到我（指定到我，或我持有该角色池的角色）。
pub async fn page_todo(
    db: &impl ConnectionTrait,
    approver_id: u64,
    req: &TodoListReq,
) -> Result<PageData<hr_approval_instance::Model>, AppError> {
    let role_ids = approval_repo::find_role_ids_by_user_id(db, approver_id).await?;
    Ok(approval_repo::find_todo_page(
        db,
        approver_id,
        &role_ids,
        req.biz_type.as_deref(),
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?)
}

/// 实例详情（实例 + 全部节点记录，按 `seq` 升序）。
pub async fn get_instance(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<(hr_approval_instance::Model, Vec<hr_approval_record::Model>), AppError> {
    let instance = approval_repo::find_instance_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("审批实例不存在：{id}")))?;
    let records = approval_repo::find_records_by_instance_id(db, id).await?;
    Ok((instance, records))
}

/// 节点记录分页。
pub async fn page_records(
    db: &impl ConnectionTrait,
    req: &RecordListReq,
) -> Result<PageData<hr_approval_record::Model>, AppError> {
    let filter = RecordFilter {
        instance_id: req.instance_id,
    };
    Ok(
        approval_repo::find_record_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

// —— 自持事务的对外入口（api 层调用）——

/// 创建模板（自持事务）。
pub async fn create_flow(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateFlowReq,
) -> Result<hr_approval_flow::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_flow_in_tx(&txn, actor_id, &req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 更新模板（自持事务）。
pub async fn update_flow(
    db: &DatabaseConnection,
    actor_id: u64,
    req: UpdateFlowReq,
) -> Result<hr_approval_flow::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_flow_in_tx(&txn, actor_id, &req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 删除模板（自持事务，软删）。
pub async fn delete_flow(db: &DatabaseConnection, actor_id: u64, id: u64) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_flow_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 新增 / 修改节点（自持事务）。
pub async fn upsert_flow_node(
    db: &DatabaseConnection,
    actor_id: u64,
    req: UpsertFlowNodeReq,
) -> Result<hr_approval_flow_node::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = upsert_flow_node_in_tx(&txn, actor_id, &req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 删除节点（自持事务，硬删）。
pub async fn delete_flow_node(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let _ = actor_id;
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_flow_node_in_tx(&txn, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 审批通过（自持事务）。
pub async fn approve(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
    opinion: &str,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = approve_in_tx(&txn, actor_id, id, opinion).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 审批驳回（自持事务）。
pub async fn reject(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
    opinion: &str,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = reject_in_tx(&txn, actor_id, id, opinion).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 撤销实例（自持事务，申请人）。
pub async fn cancel_instance(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = cancel_instance_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_role, sys_user_role};
    use crate::modules::biz::hr::approval::dto::{CreateFlowReq, TodoListReq, UpsertFlowNodeReq};
    use crate::modules::biz::hr::approval::{
        ACTION_APPROVED, ACTION_PENDING, ACTION_SKIPPED, INSTANCE_STATUS_APPROVED,
        INSTANCE_STATUS_CANCELED, INSTANCE_STATUS_PENDING, NODE_TYPE_MANAGER, NODE_TYPE_ROLE,
        NODE_TYPE_USER, repo,
    };
    use crate::modules::biz::hr::time_off::{REQUEST_STATUS_APPROVED, repo as time_off_repo};
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 测试统一用种子 admin（id = 1）作操作人（它绑定了 `super` 角色，角色池用例依赖这点）。
    const ACTOR_ID: u64 = 1;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 唯一后缀：同进程并行用例必须互不相同（撞 `uk_*_biz_type` 等唯一键）。
    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// 唯一「用户 ID」段位 900_4xx：与 employee(900_1xx) / time_off repo(900_2xx) /
    /// time_off service(900_3xx) 错开，避免同进程并行撞唯一键。
    fn unique_user_id() -> u64 {
        900_400_000
            + (std::process::id() as u64 % 100) * 1_000
            + SEQ.fetch_add(1, Ordering::Relaxed) % 1_000
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

    /// 建一个唯一业务类型的模板 + 指定节点，返回 `(flow_id, biz_type)`。
    ///
    /// `create_flow_in_tx` 不做字典校验（那在 `validate.rs`），所以状态机用例可以用假业务类型；
    /// 只有**终态分派**必须用真业务类型（见 `replace_nodes` 的说明）。
    async fn seed_flow(
        txn: &DatabaseTransaction,
        prefix: &str,
        nodes: &[(i32, i8, u64, i8)],
    ) -> (u64, String) {
        let biz_type = unique(prefix);
        let flow = super::create_flow_in_tx(
            txn,
            ACTOR_ID,
            &CreateFlowReq {
                biz_type: biz_type.clone(),
                name: format!("测试流 {biz_type}"),
                status: 1,
                remark: String::new(),
            },
        )
        .await
        .unwrap();
        replace_nodes(txn, flow.id, nodes).await;
        (flow.id, biz_type)
    }

    /// 把某模板的节点整组替换成给定形状（本事务内，随回滚复原）。
    async fn replace_nodes(txn: &DatabaseTransaction, flow_id: u64, nodes: &[(i32, i8, u64, i8)]) {
        for node in repo::find_nodes_by_flow_id(txn, flow_id).await.unwrap() {
            repo::delete_node_in_tx(txn, node.id).await.unwrap();
        }
        for (seq, node_type, approver_ref_id, skip_if_empty) in nodes {
            repo::create_node_in_tx(
                txn,
                crate::entity::hr_approval_flow_node::ActiveModel {
                    flow_id: Set(flow_id),
                    seq: Set(*seq),
                    node_name: Set(format!("节点 {seq}")),
                    node_type: Set(*node_type),
                    approver_ref_id: Set(*approver_ref_id),
                    skip_if_empty: Set(*skip_if_empty),
                    ..Default::default()
                },
                ACTOR_ID,
            )
            .await
            .unwrap();
        }
    }

    /// 终态分派会回调 `hr/time-off` 的 hook —— 预置一条**已通过**的请假单使额度分支 no-op，
    /// 本模块因此只验证审批状态机本身（额度联动由 time_off 域用例端到端钉住）。
    async fn seed_settled_request(txn: &DatabaseTransaction) -> u64 {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        let request = time_off_repo::create_request_in_tx(
            txn,
            crate::entity::hr_time_off_request::ActiveModel {
                employee_id: Set(900_400_999),
                time_off_type_id: Set(1),
                start_at: Set(day.and_hms_opt(9, 0, 0).unwrap()),
                end_at: Set(day.and_hms_opt(18, 0, 0).unwrap()),
                duration_minutes: Set(480),
                status: Set(REQUEST_STATUS_APPROVED),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap();
        request.id
    }

    /// 终态分派回调 `hr/overtime` 的 hook —— 预置一条**已通过**的加班单使入账分支 no-op。
    async fn seed_settled_overtime(txn: &DatabaseTransaction) -> u64 {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 3, 4).unwrap();
        crate::modules::biz::hr::overtime::repo::create_overtime_in_tx(
            txn,
            crate::entity::hr_overtime_request::ActiveModel {
                employee_id: Set(900_400_998),
                work_date: Set(day),
                start_at: Set(day.and_hms_opt(19, 0, 0).unwrap()),
                end_at: Set(day.and_hms_opt(21, 0, 0).unwrap()),
                duration_minutes: Set(120),
                overtime_type: Set(1),
                comp_mode: Set(2),
                status: Set(2),
                ..Default::default()
            },
            ACTOR_ID,
        )
        .await
        .unwrap()
        .id
    }

    /// 取种子里的 `super` 角色 id（角色池节点用），并确认 admin 已绑定该角色。
    async fn super_role_id(txn: &DatabaseTransaction) -> u64 {
        let role = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq("super"))
            .one(txn)
            .await
            .unwrap()
            .expect("种子 super 角色必须存在");
        assert!(
            sys_user_role::Entity::find()
                .filter(sys_user_role::Column::UserId.eq(ACTOR_ID))
                .filter(sys_user_role::Column::RoleId.eq(role.id))
                .one(txn)
                .await
                .unwrap()
                .is_some(),
            "种子 admin 应绑定 super 角色（角色池待办依赖）"
        );
        role.id
    }

    /// 确保 `timeOff` 审批流存在（种子已建则**原样复用**）。
    ///
    /// 终态分派按 `biz_type` 硬匹配 `timeOff` / `overtime`，所以这类用例只能用真业务类型；
    /// 而 `biz_type` 单列唯一 ⇒ 同一业务类型只有一条模板。**本函数刻意不改动已存在的模板节点**：
    /// time_off 域的请假单用例与审批域用例会并发跑在同一个测试二进制里，谁改节点谁就制造行锁竞争。
    /// 用例要自定义形状时，请另开模板（假业务类型）或用下面的 `replace_nodes` + 独占模板。
    async fn ensure_time_off_flow(txn: &DatabaseTransaction) -> u64 {
        match repo::find_enabled_flow_by_biz_type(txn, BIZ_TYPE_TIME_OFF)
            .await
            .unwrap()
        {
            Some(flow) => flow.id,
            None => {
                let flow = super::create_flow_in_tx(
                    txn,
                    ACTOR_ID,
                    &CreateFlowReq {
                        biz_type: BIZ_TYPE_TIME_OFF.to_string(),
                        name: "请假审批流".to_string(),
                        status: 1,
                        remark: String::new(),
                    },
                )
                .await
                .unwrap();
                // 与种子同形状：第 1 节点「直属上级」可跳过，第 2 节点「部门负责人」不可跳过
                replace_nodes(
                    txn,
                    flow.id,
                    &[
                        (1, NODE_TYPE_MANAGER, 0, 1),
                        (
                            2,
                            crate::modules::biz::hr::approval::NODE_TYPE_DEPT_LEADER,
                            0,
                            0,
                        ),
                    ],
                )
                .await;
                flow.id
            }
        }
    }

    /// 建一个**真实存在且启用**的用户（`node_type = 3 指定用户` 会校验用户存在性），返回其 id。
    async fn seed_user(txn: &DatabaseTransaction) -> u64 {
        crate::entity::sys_user::ActiveModel {
            username: Set(unique("appr_user")),
            password: Set("x".to_string()),
            emp_no: Set(String::new()),
            nickname: Set("审批人".to_string()),
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

    /// 申请人档案（`user_id = applicant_user`，直属上级 = `manager_employee`）。
    async fn seed_employee(
        txn: &DatabaseTransaction,
        user_id: u64,
        manager_employee_id: u64,
    ) -> u64 {
        crate::entity::hr_employee::ActiveModel {
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

    /// 造一对「申请人 + 直属上级」，并让**同一个上级**同时是该申请人主部门的负责人。
    ///
    /// 这样种子形状的 `timeOff` 流（直属上级 → 部门负责人）两级都会解析到同一个审批人，
    /// 用例无需改动共享模板即可走完两级审批。
    async fn seed_subordinate_with_leader(txn: &DatabaseTransaction) -> (u64, u64, u64) {
        let manager_user = unique_user_id();
        let applicant_user = unique_user_id();
        let dept_id = unique_user_id();

        let manager_employee = seed_employee(txn, manager_user, 0).await;
        let applicant_employee = seed_employee(txn, applicant_user, manager_employee).await;

        for (user_id, is_primary, is_leader) in [(applicant_user, 1, 0), (manager_user, 0, 1)] {
            crate::entity::sys_user_dept::ActiveModel {
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

    /// 提交时展开节点：解析不到审批人且该节点 `skip_if_empty = 1` → 记跳过，
    /// `current_seq` 直接落在第一个待审批节点上。
    #[tokio::test]
    async fn start_instance_skips_unresolvable_optional_node() {
        let txn = test_txn().await;
        // 申请人是**新建的唯一用户、没有员工档案**（不能复用 admin：开发库可能已给 admin 建过档案，
        // 那样「直属上级」就能解析到人，本用例的前提失效）→ 该节点解析不到但允许跳过
        let applicant = unique_user_id();
        let (_flow_id, biz_type) = seed_flow(
            &txn,
            "approval_skip",
            &[
                (1, NODE_TYPE_MANAGER, 0, 1),
                (2, NODE_TYPE_USER, ACTOR_ID, 0),
            ],
        )
        .await;

        let instance_id =
            super::start_instance_in_tx(&txn, applicant, &biz_type, 900_400_001, applicant)
                .await
                .unwrap();
        let (instance, records) = super::get_instance(&txn, instance_id).await.unwrap();

        assert_eq!(instance.status, INSTANCE_STATUS_PENDING, "应处于审批中");
        assert_eq!(
            instance.current_seq, 2,
            "current_seq 应落在跳过后的第一个节点"
        );
        assert_eq!(
            instance.current_approver_id, ACTOR_ID,
            "当前节点审批人应解析为指定用户"
        );
        assert_eq!(records.len(), 2, "节点记录在提交时一次展开");
        assert_eq!(
            records[0].action, ACTION_SKIPPED,
            "解析不到且可跳过 → 记跳过"
        );
        assert_eq!(records[1].action, ACTION_PENDING);
    }

    /// 最后一个节点不允许跳过：解析不到审批人必须报错，而不是让单据无人把关。
    #[tokio::test]
    async fn start_instance_rejects_when_last_node_has_no_approver() {
        let txn = test_txn().await;
        // 同上：申请人必须是没有员工档案的唯一用户，否则「直属上级」会解析到人
        let applicant = unique_user_id();
        let (_flow_id, biz_type) =
            seed_flow(&txn, "approval_last", &[(1, NODE_TYPE_MANAGER, 0, 0)]).await;

        let err = super::start_instance_in_tx(&txn, applicant, &biz_type, 900_400_002, applicant)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("没有可用的审批人"),
            "末节点解析不到审批人时必须报错，实际：{err}"
        );
    }

    /// 两节点顺序审批：第一节点通过 → 推进；第二节点通过 → 实例终态；终态再点必须被拒；
    /// 同一单据再次启动（重提交）→ 另起一条新实例。
    #[tokio::test]
    async fn approve_advances_then_finishes_and_restart_creates_new_instance() {
        let txn = test_txn().await;
        let biz_id = seed_settled_request(&txn).await;
        ensure_time_off_flow(&txn).await;
        let (_applicant_employee, applicant_user, manager_user) =
            seed_subordinate_with_leader(&txn).await;

        let instance_id = super::start_instance_in_tx(
            &txn,
            applicant_user,
            BIZ_TYPE_TIME_OFF,
            biz_id,
            applicant_user,
        )
        .await
        .unwrap();
        let (start, _) = super::get_instance(&txn, instance_id).await.unwrap();
        assert_eq!(
            start.current_approver_id, manager_user,
            "第 1 节点「直属上级」应解析到申请人的上级"
        );

        super::approve_in_tx(&txn, manager_user, instance_id, "同意")
            .await
            .unwrap();
        let (advanced, _) = super::get_instance(&txn, instance_id).await.unwrap();
        assert_eq!(
            advanced.status, INSTANCE_STATUS_PENDING,
            "第一个节点通过后仍在审批中"
        );
        assert_eq!(advanced.current_seq, 2, "应推进到第二个节点");
        assert_eq!(
            advanced.current_approver_id, manager_user,
            "第 2 节点「部门负责人」应解析到主部门负责人"
        );

        super::approve_in_tx(&txn, manager_user, instance_id, "同意")
            .await
            .unwrap();
        let (finished, records) = super::get_instance(&txn, instance_id).await.unwrap();
        assert_eq!(
            finished.status, INSTANCE_STATUS_APPROVED,
            "末节点通过后实例应已通过"
        );
        assert_eq!(finished.current_seq, 0, "终态应清空当前节点");
        assert_eq!(finished.current_approver_id, 0, "终态应清空当前审批人");
        assert!(finished.finished_at.is_some(), "终态必须写 finished_at");
        assert!(
            records.iter().all(|r| r.action == ACTION_APPROVED),
            "全部节点记录都应为通过"
        );

        let err = super::approve_in_tx(&txn, manager_user, instance_id, "")
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("已完成审批"),
            "终态实例重复审批必须被拒，实际：{err}"
        );

        let restarted = super::start_instance_in_tx(
            &txn,
            applicant_user,
            BIZ_TYPE_TIME_OFF,
            biz_id,
            applicant_user,
        )
        .await
        .unwrap();
        assert_ne!(restarted, instance_id, "终态后重提交必须新建实例");
        let (fresh, _) = super::get_instance(&txn, restarted).await.unwrap();
        assert_eq!(fresh.status, INSTANCE_STATUS_PENDING);

        let again = super::start_instance_in_tx(
            &txn,
            applicant_user,
            BIZ_TYPE_TIME_OFF,
            biz_id,
            applicant_user,
        )
        .await
        .unwrap();
        assert_eq!(again, restarted, "审批中重复提交应复用同一实例（幂等）");
    }

    /// 非当前节点审批人操作必须被拒。
    #[tokio::test]
    async fn approve_rejects_when_actor_is_not_current_approver() {
        let txn = test_txn().await;
        // node_type = 3 会校验「指定的审批用户存在且启用」，故这里必须建真用户
        let other = seed_user(&txn).await;
        let (_flow_id, biz_type) =
            seed_flow(&txn, "approval_actor", &[(1, NODE_TYPE_USER, other, 0)]).await;

        let instance_id =
            super::start_instance_in_tx(&txn, ACTOR_ID, &biz_type, 900_400_003, ACTOR_ID)
                .await
                .unwrap();
        let err = super::approve_in_tx(&txn, ACTOR_ID, instance_id, "")
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("不由你审批"),
            "非当前审批人必须被拒，实际：{err}"
        );
    }

    /// 角色池节点：持有该角色的用户在「我的待办」里看得到，且节点只生效一次。
    ///
    /// 用 **overtime** 业务类型承载（`timeOff` 的模板被 time_off 域用例共享，本用例需要自定义
    /// 「角色池」节点形状，故独占一条模板，避免并发改同一批节点行）。
    #[tokio::test]
    async fn role_pool_node_appears_in_todo_and_can_be_approved_once() {
        let txn = test_txn().await;
        let role_id = super_role_id(&txn).await;
        let biz_id = seed_settled_overtime(&txn).await;

        let flow_id = match repo::find_enabled_flow_by_biz_type(&txn, BIZ_TYPE_OVERTIME)
            .await
            .unwrap()
        {
            Some(flow) => flow.id,
            None => {
                super::create_flow_in_tx(
                    &txn,
                    ACTOR_ID,
                    &CreateFlowReq {
                        biz_type: BIZ_TYPE_OVERTIME.to_string(),
                        name: "加班审批流".to_string(),
                        status: 1,
                        remark: String::new(),
                    },
                )
                .await
                .unwrap()
                .id
            }
        };
        replace_nodes(&txn, flow_id, &[(1, NODE_TYPE_ROLE, role_id, 0)]).await;

        let instance_id =
            super::start_instance_in_tx(&txn, ACTOR_ID, BIZ_TYPE_OVERTIME, biz_id, ACTOR_ID)
                .await
                .unwrap();
        let (instance, _) = super::get_instance(&txn, instance_id).await.unwrap();
        assert_eq!(instance.current_approver_id, 0, "角色池不锁定具体审批人");
        assert_eq!(instance.current_approver_role_id, role_id);

        let todo = super::page_todo(
            &txn,
            ACTOR_ID,
            &TodoListReq {
                page: crate::utils::PageQuery {
                    page: Some(1),
                    page_size: Some(100),
                },
                biz_type: Some(BIZ_TYPE_OVERTIME.to_string()),
            },
        )
        .await
        .unwrap();
        assert!(
            todo.items.iter().any(|item| item.id == instance_id),
            "持有该角色的用户应在待办里看到该实例"
        );

        super::approve_in_tx(&txn, ACTOR_ID, instance_id, "同意")
            .await
            .unwrap();
        let (done, records) = super::get_instance(&txn, instance_id).await.unwrap();
        assert_eq!(done.status, INSTANCE_STATUS_APPROVED);
        assert_eq!(records[0].acted_by, ACTOR_ID, "角色池节点应记实际操作人");
    }

    /// 撤销：只有申请人本人可撤销，撤销后实例终态为已撤销。
    #[tokio::test]
    async fn cancel_rejects_non_applicant_and_finishes_as_canceled() {
        let txn = test_txn().await;
        let biz_id = seed_settled_request(&txn).await;
        ensure_time_off_flow(&txn).await;
        let (_applicant_employee, applicant_user, _manager_user) =
            seed_subordinate_with_leader(&txn).await;

        let instance_id = super::start_instance_in_tx(
            &txn,
            applicant_user,
            BIZ_TYPE_TIME_OFF,
            biz_id,
            applicant_user,
        )
        .await
        .unwrap();

        let err = super::cancel_instance_in_tx(&txn, ACTOR_ID, instance_id)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("只有申请人"),
            "非申请人撤销必须被拒，实际：{err}"
        );

        super::cancel_instance_in_tx(&txn, applicant_user, instance_id)
            .await
            .unwrap();
        let (instance, _) = super::get_instance(&txn, instance_id).await.unwrap();
        assert_eq!(instance.status, INSTANCE_STATUS_CANCELED);
        assert!(instance.finished_at.is_some(), "撤销也要写终态时间");
    }

    /// 模板删除护栏：有审批中的实例时拒绝删除（在途单据会失去推进依据）。
    #[tokio::test]
    async fn delete_flow_rejects_when_pending_instance_exists() {
        let txn = test_txn().await;
        let (flow_id, biz_type) = seed_flow(
            &txn,
            "approval_delflow",
            &[(1, NODE_TYPE_USER, ACTOR_ID, 0)],
        )
        .await;
        let _ = super::start_instance_in_tx(&txn, ACTOR_ID, &biz_type, 900_400_004, ACTOR_ID)
            .await
            .unwrap();

        let err = super::delete_flow_in_tx(&txn, ACTOR_ID, flow_id)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("审批中的单据"),
            "有在途实例时不得删除模板，实际：{err}"
        );
    }

    /// 模板节点：同一模板内顺序号唯一；最后一个节点不允许配「可跳过」。
    #[tokio::test]
    async fn upsert_node_rejects_duplicate_seq_and_skippable_last_node() {
        let txn = test_txn().await;
        let (flow_id, _biz_type) =
            seed_flow(&txn, "approval_node", &[(1, NODE_TYPE_USER, ACTOR_ID, 0)]).await;

        let duplicate = super::upsert_flow_node_in_tx(
            &txn,
            ACTOR_ID,
            &UpsertFlowNodeReq {
                id: 0,
                flow_id,
                seq: 1,
                node_name: "重复顺序号".into(),
                node_type: NODE_TYPE_USER,
                approver_ref_id: ACTOR_ID,
                skip_if_empty: 0,
                remark: String::new(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            duplicate.to_string().contains("顺序号已被占用"),
            "重复 seq 必须被拒，实际：{duplicate}"
        );

        let skippable_last = super::upsert_flow_node_in_tx(
            &txn,
            ACTOR_ID,
            &UpsertFlowNodeReq {
                id: 0,
                flow_id,
                seq: 2,
                node_name: "可跳过的末节点".into(),
                node_type: NODE_TYPE_USER,
                approver_ref_id: ACTOR_ID,
                skip_if_empty: 1,
                remark: String::new(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            skippable_last.to_string().contains("不允许跳过"),
            "最后一个节点不允许跳过，实际：{skippable_last}"
        );
    }

    /// 删除节点后新的末节点若配了「可跳过」也要被拒（否则删节点会绕过规则）。
    #[tokio::test]
    async fn delete_node_rejects_when_new_last_node_is_skippable() {
        let txn = test_txn().await;
        let (flow_id, _biz_type) = seed_flow(
            &txn,
            "approval_delnode",
            &[
                (1, NODE_TYPE_USER, ACTOR_ID, 1),
                (2, NODE_TYPE_USER, ACTOR_ID, 0),
            ],
        )
        .await;

        let nodes = repo::find_nodes_by_flow_id(&txn, flow_id).await.unwrap();
        let last = nodes.last().unwrap();
        let err = super::delete_flow_node_in_tx(&txn, last.id)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("不允许跳过"),
            "删除后末节点可跳过必须被拒，实际：{err}"
        );
    }

    /// 模板业务类型单列唯一（**含软删占位**）：软删后仍不能再建同类型模板。
    #[tokio::test]
    async fn create_flow_rejects_duplicate_biz_type_including_soft_deleted() {
        let txn = test_txn().await;
        let (flow_id, biz_type) =
            seed_flow(&txn, "approval_dup", &[(1, NODE_TYPE_USER, ACTOR_ID, 0)]).await;
        super::delete_flow_in_tx(&txn, ACTOR_ID, flow_id)
            .await
            .unwrap();

        let err = super::create_flow_in_tx(
            &txn,
            ACTOR_ID,
            &CreateFlowReq {
                biz_type: biz_type.clone(),
                name: "重复".into(),
                status: 1,
                remark: String::new(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("已存在审批流"),
            "软删行仍占唯一键，必须查重，实际：{err}"
        );
    }
}
