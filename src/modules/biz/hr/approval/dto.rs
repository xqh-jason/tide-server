//! 审批域 DTO：模板 / 模板节点 / 实例 / 节点记录的请求与响应体声明。
//!
//! 约定同其它域：entity 不直接暴露给接口，响应体一律经 `From<Model>` 转换；
//! 请求体永不接受 `*_by` / `*_name`；时间字段一律 `String`（DTO 不做解析）；
//! `created_by_name` / `updated_by_name` / `applicant_name` / `approver_name` / `acted_by_name`
//! 由平台唯一管道 `utils::user_ref::fill_user_names` 经 `UserRefNames` 填充
//! （`From<Model>` 里一律留空串）。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{
    hr_approval_flow, hr_approval_flow_node, hr_approval_instance, hr_approval_record,
};
use crate::utils::PageQuery;
use crate::utils::serde_format::format_datetime;
use crate::utils::user_ref::UserRefNames;

/// `Option<DateTime>` → `Option<String>`（`yyyy-MM-dd HH:mm:ss`）。
fn fmt_datetime_opt(t: Option<chrono::NaiveDateTime>) -> Option<String> {
    t.map(format_datetime)
}

// —— 模板 ——

/// 审批流模板列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配业务类型 / 模板名称）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤（1 启用 0 停用）；不传查全部
    pub status: Option<i8>,
}

/// 审批流模板分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct FlowFilter {
    /// 模糊搜索关键字（匹配业务类型 / 模板名称）
    pub keyword: Option<String>,
    /// 状态精确过滤
    pub status: Option<i8>,
}

/// 创建审批流模板请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFlowReq {
    /// 业务类型（字典 approvalBizType，如 timeOff / overtime；单列唯一含软删占位）
    pub biz_type: String,
    /// 模板名称
    pub name: String,
    /// 状态：1 启用 0 停用
    pub status: i8,
    /// 备注
    pub remark: String,
}

/// 更新审批流模板请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFlowReq {
    /// 模板主键
    pub id: u64,
    /// 业务类型（唯一，变更时查重）
    pub biz_type: String,
    /// 模板名称
    pub name: String,
    /// 状态：1 启用 0 停用
    pub status: i8,
    /// 备注
    pub remark: String,
}

/// 审批流模板响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowResp {
    pub id: u64,
    /// 业务类型
    pub biz_type: String,
    /// 模板名称
    pub name: String,
    /// 状态：1 启用 0 停用
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

impl From<hr_approval_flow::Model> for FlowResp {
    fn from(m: hr_approval_flow::Model) -> Self {
        Self {
            id: m.id,
            biz_type: m.biz_type,
            name: m.name,
            status: m.status,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

impl UserRefNames for FlowResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 模板详情响应体：模板 + 其节点（按 `seq` 升序）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowDetailResp {
    /// 模板本体
    pub flow: FlowResp,
    /// 模板节点（按 seq 升序）
    pub nodes: Vec<FlowNodeResp>,
}

// —— 模板节点 ——

/// 模板节点列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowNodeListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 审批流模板 ID
    pub flow_id: u64,
}

/// 模板节点分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct FlowNodeFilter {
    /// 审批流模板 ID
    pub flow_id: u64,
}

/// 模板节点新增 / 修改请求（`id = 0` 新增，`> 0` 修改）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertFlowNodeReq {
    /// 节点主键；0 = 新增
    #[serde(default)]
    pub id: u64,
    /// 审批流模板 ID
    pub flow_id: u64,
    /// 顺序号（从 1 递增，唯一键 `(flow_id, seq)`）
    pub seq: i32,
    /// 节点名称
    pub node_name: String,
    /// 节点类型：1 直属上级 2 部门负责人 3 指定用户 4 指定角色
    pub node_type: i8,
    /// 审批人引用 ID：node_type=3 是 sys_user.id、=4 是 sys_role.id、1/2 忽略
    #[serde(default)]
    pub approver_ref_id: u64,
    /// 解析不到审批人时是否跳过：1 跳过 0 报错（最后一个节点必须为 0）
    pub skip_if_empty: i8,
    /// 备注
    pub remark: String,
}

/// 模板节点响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowNodeResp {
    pub id: u64,
    /// 审批流模板 ID
    pub flow_id: u64,
    /// 顺序号
    pub seq: i32,
    /// 节点名称
    pub node_name: String,
    /// 节点类型：1 直属上级 2 部门负责人 3 指定用户 4 指定角色
    pub node_type: i8,
    /// 审批人引用 ID
    pub approver_ref_id: u64,
    /// 解析不到审批人时是否跳过：1 跳过 0 报错
    pub skip_if_empty: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间
    pub updated_at: String,
}

impl From<hr_approval_flow_node::Model> for FlowNodeResp {
    fn from(m: hr_approval_flow_node::Model) -> Self {
        Self {
            id: m.id,
            flow_id: m.flow_id,
            seq: m.seq,
            node_name: m.node_name,
            node_type: m.node_type,
            approver_ref_id: m.approver_ref_id,
            skip_if_empty: m.skip_if_empty,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
        }
    }
}

// —— 实例 ——

/// 审批实例列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 业务类型精确过滤（字典 approvalBizType）；不传查全部
    pub biz_type: Option<String>,
    /// 状态精确过滤（1 审批中 2 已通过 3 已驳回 4 已撤销）；不传查全部
    pub status: Option<i8>,
    /// 申请人 sys_user.id 精确过滤；不传查全部
    pub applicant_id: Option<u64>,
}

/// 审批实例分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct InstanceFilter {
    /// 业务类型精确过滤
    pub biz_type: Option<String>,
    /// 状态精确过滤
    pub status: Option<i8>,
    /// 申请人精确过滤
    pub applicant_id: Option<u64>,
}

/// 我的待办请求（审批人工作台：只看当前节点轮到自己 / 自己角色池的实例）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 业务类型精确过滤；不传查全部
    pub biz_type: Option<String>,
}

/// 审批实例响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceResp {
    pub id: u64,
    /// 业务类型（字典 approvalBizType）
    pub biz_type: String,
    /// 业务单据 ID
    pub biz_id: u64,
    /// 审批流模板 ID
    pub flow_id: u64,
    /// 申请人 sys_user.id
    pub applicant_id: u64,
    /// 当前待审批节点顺序号；0 = 无可推进节点
    pub current_seq: i32,
    /// 当前节点审批人 sys_user.id；0 = 角色池或无
    pub current_approver_id: u64,
    /// 当前节点角色池 sys_role.id；0 = 非角色池
    pub current_approver_role_id: u64,
    /// 状态：1 审批中 2 已通过 3 已驳回 4 已撤销
    pub status: i8,
    /// 终态时间（`yyyy-MM-dd HH:mm:ss`）；未终结为 null
    pub finished_at: Option<String>,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 申请人显示名（`fill_user_names` 批量拼装）
    pub applicant_name: String,
    /// 当前审批人显示名（角色池为空串）
    pub current_approver_name: String,
    /// 创建人显示名
    pub created_by_name: String,
    /// 更新人显示名
    pub updated_by_name: String,
}

impl From<hr_approval_instance::Model> for InstanceResp {
    fn from(m: hr_approval_instance::Model) -> Self {
        Self {
            id: m.id,
            biz_type: m.biz_type,
            biz_id: m.biz_id,
            flow_id: m.flow_id,
            applicant_id: m.applicant_id,
            current_seq: m.current_seq,
            current_approver_id: m.current_approver_id,
            current_approver_role_id: m.current_approver_role_id,
            status: m.status,
            finished_at: fmt_datetime_opt(m.finished_at),
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            applicant_name: String::new(),
            current_approver_name: String::new(),
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

impl UserRefNames for InstanceResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.applicant_name = names.get(&self.applicant_id).cloned().unwrap_or_default();
        self.current_approver_name = names
            .get(&self.current_approver_id)
            .cloned()
            .unwrap_or_default();
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 实例详情响应体：实例 + 全部节点记录（按 `seq` 升序）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDetailResp {
    /// 实例本体
    pub instance: InstanceResp,
    /// 节点记录（按 seq 升序）
    pub records: Vec<RecordResp>,
}

/// 审批动作请求（通过 / 驳回）：实例 ID + 审批意见。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApproveReq {
    /// 审批实例 ID
    pub id: u64,
    /// 审批意见
    #[serde(default)]
    pub opinion: String,
}

// —— 节点记录 ——

/// 节点记录列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 审批实例 ID
    pub instance_id: u64,
}

/// 节点记录分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct RecordFilter {
    /// 审批实例 ID
    pub instance_id: u64,
}

/// 节点记录响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordResp {
    pub id: u64,
    /// 审批实例 ID
    pub instance_id: u64,
    /// 节点顺序号
    pub seq: i32,
    /// 节点名称快照
    pub node_name: String,
    /// 节点类型快照：1 直属上级 2 部门负责人 3 指定用户 4 指定角色
    pub node_type: i8,
    /// 审批人 sys_user.id；0 = 角色池或解析不到
    pub approver_id: u64,
    /// 审批人引用 ID 快照（node_type=3 是 sys_user.id、=4 是 sys_role.id）
    pub approver_ref_id: u64,
    /// 动作：0 待审批 1 通过 2 驳回 3 跳过
    pub action: i8,
    /// 审批意见
    pub opinion: String,
    /// 动作时间（`yyyy-MM-dd HH:mm:ss`）；未动作为 null
    pub acted_at: Option<String>,
    /// 实际操作人 sys_user.id
    pub acted_by: u64,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 审批人显示名（角色池为空串）
    pub approver_name: String,
    /// 实际操作人显示名
    pub acted_by_name: String,
}

impl From<hr_approval_record::Model> for RecordResp {
    fn from(m: hr_approval_record::Model) -> Self {
        Self {
            id: m.id,
            instance_id: m.instance_id,
            seq: m.seq,
            node_name: m.node_name,
            node_type: m.node_type,
            approver_id: m.approver_id,
            approver_ref_id: m.approver_ref_id,
            action: m.action,
            opinion: m.opinion,
            acted_at: fmt_datetime_opt(m.acted_at),
            acted_by: m.acted_by,
            created_at: format_datetime(m.created_at),
            approver_name: String::new(),
            acted_by_name: String::new(),
        }
    }
}

impl UserRefNames for RecordResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.approver_name = names.get(&self.approver_id).cloned().unwrap_or_default();
        self.acted_by_name = names.get(&self.acted_by).cloned().unwrap_or_default();
    }
}
