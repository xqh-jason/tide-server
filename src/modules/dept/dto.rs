//! 部门 DTO（传输对象）：entity 不直接暴露给接口，经 From 转换。
//!
//! 校验约定：请求体的值域校验**不写在 DTO 文件里**，见同模块 `validate.rs` 中
//! 手写的 `validate_*` 函数（规则与错误文案按字段分组，字段在此保持纯声明）。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::entity::sys_dept;
use crate::utils::user_ref::UserRefNames;

/// 部门负责人展示项（来源 `sys_user_dept.is_leader = 1`，允许多个）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeptLeader {
    /// 负责人用户 id（`sys_user.id`）
    pub user_id: u64,
    /// 负责人显示名（`sys_user.username`；查不到给空串）
    pub user_name: String,
}

/// 部门响应体（树节点：`children` 递归，为空时序列化省略）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeptResp {
    /// 部门 id
    pub id: u64,
    /// 父部门 id，`0` 表示根部门
    pub parent_id: u64,
    /// 部门路径（根 `/0/{id}/`，子 `/0/父id/{id}/`）
    pub dept_path: String,
    /// 部门名（同父唯一）
    pub dept_name: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 同级互看：`1` 同部门普通成员可互看、`0` 关
    pub allow_peer_read: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 创建人 ID（`sys_user.id`；`0` 表示种子/系统写入）
    pub created_by: u64,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: u64,
    /// 创建人显示名（`sys_user.username`）
    pub created_by_name: String,
    /// 更新人显示名（`sys_user.username`）
    pub updated_by_name: String,
    /// 部门负责人列表（`is_leader = 1`；为空时序列化省略）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub leaders: Vec<DeptLeader>,
    /// 子部门；为空时序列化省略该字段
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DeptResp>,
}

/// `sys_dept::Model` → `DeptResp` 字段搬运（children 由 service 组装）。
impl From<sys_dept::Model> for DeptResp {
    fn from(m: sys_dept::Model) -> Self {
        Self {
            id: m.id,
            parent_id: m.parent_id,
            dept_path: m.dept_path,
            dept_name: m.dept_name,
            sort: m.sort,
            status: m.status,
            allow_peer_read: m.allow_peer_read,
            remark: m.remark,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
            leaders: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// 按名称映射填充 `DeptResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for DeptResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 创建部门请求：`parent_id = 0` 表示根部门。
///
/// 值域校验见同模块 `validate.rs` 中 `validate_create_dept`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeptReq {
    /// 父部门 id，`0` 表示根部门
    pub parent_id: u64,
    /// 部门名（同父唯一，含软删占位）
    pub dept_name: String,
    /// 排序值，越小越靠前；无特殊排序传 `0`
    pub sort: i32,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 同级互看：`1` 开启、`0` 关闭
    pub allow_peer_read: i8,
    /// 备注，无备注传空串
    pub remark: String,
}

/// 更新部门请求（编辑表单全量提交）：`parent_id` 变更即移动子树。
///
/// 值域校验见同模块 `validate.rs` 中 `validate_update_dept`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeptReq {
    /// 目标部门 id
    pub id: u64,
    /// 父部门 id，`0` 表示根部门；与原值不同即触发子树移动
    pub parent_id: u64,
    /// 部门名（同父唯一，排除自身查重）
    pub dept_name: String,
    /// 排序值
    pub sort: i32,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 同级互看：`1` 开启、`0` 关闭
    pub allow_peer_read: i8,
    /// 备注
    pub remark: String,
}
