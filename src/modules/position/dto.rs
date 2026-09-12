//! 职位 DTO：entity 不直接暴露给接口，经 From 转换。
//!
//! 校验约定：请求体的值域校验**不写在 DTO 文件里**，见同模块 `validate.rs` 中
//! 手写的 `validate_*` 函数（规则与错误文案按字段分组，字段在此保持纯声明）。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_position;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

/// 职位响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PositionResp {
    /// 职位 id
    pub id: u64,
    /// 职位编码（全局唯一，含软删占位）
    pub position_code: String,
    /// 职位名称（显示名）
    pub position_name: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
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
    /// 创建人姓名（显示名）
    pub created_by_name: String,
    /// 更新人姓名（显示名）
    pub updated_by_name: String,
}

/// `sys_position::Model` → `PositionResp` 字段搬运。
impl From<sys_position::Model> for PositionResp {
    fn from(m: sys_position::Model) -> Self {
        Self {
            id: m.id,
            position_code: m.position_code,
            position_name: m.position_name,
            sort: m.sort,
            status: m.status,
            remark: m.remark,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充 `PositionResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for PositionResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 职位列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PositionListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配编码 / 名称）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 禁用；不传查全部
    pub status: Option<i8>,
    /// 创建人 ID 精确过滤（前端用户选择器回填 id）；不传查全部
    pub created_by: Option<u64>,
    /// 更新人 ID 精确过滤；不传查全部
    pub updated_by: Option<u64>,
    /// 创建时间范围起（`yyyy-MM-dd[ HH:mm:ss]`，含边界）；不传查全部
    pub created_at_begin: Option<String>,
    /// 创建时间范围止（含边界）；不传查全部
    pub created_at_end: Option<String>,
    /// 更新时间范围起（同上格式）；不传查全部
    pub updated_at_begin: Option<String>,
    /// 更新时间范围止（含边界）；不传查全部
    pub updated_at_end: Option<String>,
}

/// 职位分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct PositionFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 创建职位请求。
///
/// 值域校验见同模块 `validate.rs` 中 `validate_create_position`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePositionReq {
    /// 职位编码（全局唯一，含软删占位）
    pub position_code: String,
    /// 职位名称（显示名）
    pub position_name: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 备注，无备注传空串
    pub remark: String,
}

/// 更新职位请求（编辑表单全量提交）。
///
/// 值域校验见同模块 `validate.rs` 中 `validate_update_position`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePositionReq {
    /// 目标职位 id
    pub id: u64,
    /// 职位编码（全局唯一，排除自身查重）
    pub position_code: String,
    /// 职位名称
    pub position_name: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 备注，无备注传空串
    pub remark: String,
}
