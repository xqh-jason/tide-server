//! HR 审批基座域：模板（谁审）→ 实例（这单走到哪）→ 节点记录（每步结论）。
//!
//! 四张表：`hr_approval_flow`（模板，软删）/ `hr_approval_flow_node`（模板节点，硬删）/
//! `hr_approval_instance`（实例）/ `hr_approval_record`（节点记录，提交时一次展开）。
//!
//! 跨域契约（`hr/time-off` 与 `hr/overtime` 直接调用 `service`，不各自复制审批逻辑）：
//! - [`service::start_instance_in_tx`]：提交单据时启动审批（同事务）；
//! - [`service::approve_in_tx`] / [`service::reject_in_tx`] / [`service::cancel_instance_in_tx`]：
//!   推进实例；
//! - 终态由本域按 `biz_type` **match 分派**到业务域 `on_instance_finished_in_tx`
//!   （单向调用：业务域提交 → 本域推进 → 业务域终态副作用；同 crate 内的模块互引，
//!   没有注册表 / trait 抽象，也没有运行期查找）。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

// —— 审批域常量唯一定义点（复制到别处即漂移）——

/// 节点类型：直属上级（`hr_employee.manager_employee_id` → 其 `user_id`）。
pub const NODE_TYPE_MANAGER: i8 = 1;
/// 节点类型：部门负责人（申请人**主部门**的 `is_leader = 1` 用户，多个取 `user_id` 最小）。
pub const NODE_TYPE_DEPT_LEADER: i8 = 2;
/// 节点类型：指定用户（`approver_ref_id` = `sys_user.id`）。
pub const NODE_TYPE_USER: i8 = 3;
/// 节点类型：指定角色（`approver_ref_id` = `sys_role.id`，`approver_id = 0` 表示角色池）。
pub const NODE_TYPE_ROLE: i8 = 4;

/// 节点动作：待审批。
pub const ACTION_PENDING: i8 = 0;
/// 节点动作：通过。
pub const ACTION_APPROVED: i8 = 1;
/// 节点动作：驳回。
pub const ACTION_REJECTED: i8 = 2;
/// 节点动作：跳过（解析不到审批人且该节点 `skip_if_empty = 1`）。
pub const ACTION_SKIPPED: i8 = 3;

/// 实例状态：审批中。
pub const INSTANCE_STATUS_PENDING: i8 = 1;
/// 实例状态：已通过。
pub const INSTANCE_STATUS_APPROVED: i8 = 2;
/// 实例状态：已驳回。
pub const INSTANCE_STATUS_REJECTED: i8 = 3;
/// 实例状态：已撤销（申请人撤销 / 单据软删）。
pub const INSTANCE_STATUS_CANCELED: i8 = 4;

/// 模板状态：启用。
pub const FLOW_STATUS_ENABLED: i8 = 1;

/// 业务类型字典类型编码（`sys_dictionary.type`；业务类型允许值的唯一来源）。
pub const DICT_APPROVAL_BIZ_TYPE: &str = "approvalBizType";

/// 业务类型：请假单（字典 `approvalBizType`）。
pub const BIZ_TYPE_TIME_OFF: &str = "timeOff";
/// 业务类型：加班单（字典 `approvalBizType`）。
pub const BIZ_TYPE_OVERTIME: &str = "overtime";

/// 审批模板里的节点数上限（一次展开的节点记录数，防止误配出超长链）。
pub const MAX_FLOW_NODES: u64 = 20;

/// 当前节点无可推进的哨兵值（实例 `current_seq`）。
pub const CURRENT_SEQ_NONE: i32 = 0;

/// 审批域端点：`POST /api/v1/hr/approval/{flow,flow-node,instance,record}/...`。
///
/// 路由在此注册，由 `modules/mod.rs` 的 `DOMAINS` 登记表挂到 `hr/approval` 前缀下
/// （`MountGuard::Protected` → 自动获得 AuthRequired + OperationLog + ApiPermission 三件套）；
/// 每个端点都必须登记进 `sys_api`，漏登 = 该端点 fail-open（见 seed.rs 的 API_SEEDS）。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["审批基座"])
        .push(
            Router::with_path("flow")
                .push(Router::with_path("list").post(api::list_flows))
                .push(Router::with_path("create").post(api::create_flow))
                .push(Router::with_path("update").post(api::update_flow))
                .push(Router::with_path("get").post(api::get_flow))
                .push(Router::with_path("delete").post(api::delete_flow)),
        )
        .push(
            Router::with_path("flow-node")
                .push(Router::with_path("list").post(api::list_flow_nodes))
                .push(Router::with_path("upsert").post(api::upsert_flow_node))
                .push(Router::with_path("delete").post(api::delete_flow_node)),
        )
        .push(
            Router::with_path("instance")
                .push(Router::with_path("list").post(api::list_instances))
                .push(Router::with_path("get").post(api::get_instance))
                .push(Router::with_path("todo").post(api::list_todo_instances))
                .push(Router::with_path("approve").post(api::approve_instance))
                .push(Router::with_path("reject").post(api::reject_instance))
                .push(Router::with_path("cancel").post(api::cancel_instance)),
        )
        .push(Router::with_path("record").push(Router::with_path("list").post(api::list_records)))
}
