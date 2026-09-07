//! 数据字典 DTO：entity 不直接暴露给接口，经 From 转换。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

// —— 字典类型 ——

/// 字典类型响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryResp {
    /// 字典类型 id
    pub id: u64,
    /// 类型名称（显示名）
    pub name: String,
    /// 类型编码（全局唯一，含软删占位）
    pub r#type: String,
    /// 状态：`1` 启用、`0` 停用
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

/// `sys_dictionary::Model` → `DictionaryResp` 字段搬运。
impl From<sys_dictionary::Model> for DictionaryResp {
    fn from(m: sys_dictionary::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            r#type: m.r#type,
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

/// 按名称映射填充 `DictionaryResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for DictionaryResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 字典类型列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配 name / type）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 停用；不传查全部
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

/// 字典类型分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct DictionaryFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 创建字典类型请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDictionaryReq {
    /// 类型名称（显示名）
    pub name: String,
    /// 类型编码（全局唯一，含软删占位）
    pub r#type: String,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 备注，可空
    pub remark: Option<String>,
}

/// 更新字典类型请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictionaryReq {
    /// 目标字典类型 id
    pub id: u64,
    /// 类型名称
    pub name: String,
    /// 类型编码（全局唯一，排除自身查重）
    pub r#type: String,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
    /// 备注，可空
    pub remark: Option<String>,
}

// —— get-by-type 下拉契约 ——

/// 按类型编码取启用字典项的请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryTypeReq {
    /// 字典类型编码（类型不存在或已停用返回业务错误）
    pub r#type: String,
}

/// `get-by-type` 响应：类型信息 + 启用字典项（前端下拉一次拿全）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryOptionResp {
    /// 字典类型 id
    pub id: u64,
    /// 类型名称
    pub name: String,
    /// 类型编码
    pub r#type: String,
    /// 启用的字典项列表（按 sort 升序）
    pub details: Vec<DictionaryDetailOption>,
}

/// 下拉项：只带前端渲染需要的字段。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryDetailOption {
    /// 字典项 id
    pub id: u64,
    /// 显示文本
    pub label: String,
    /// 选项值
    pub value: String,
    /// 扩展字段（JSON 字符串，业务自定义）
    pub extend: String,
    /// 排序值，越小越靠前
    pub sort: i32,
}

/// `sys_dictionary_detail::Model` → `DictionaryDetailOption` 字段搬运。
impl From<sys_dictionary_detail::Model> for DictionaryDetailOption {
    fn from(m: sys_dictionary_detail::Model) -> Self {
        Self {
            id: m.id,
            label: m.label,
            value: m.value,
            extend: m.extend,
            sort: m.sort,
        }
    }
}

// —— 字典项 ——

/// 字典项响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryDetailResp {
    /// 字典项 id
    pub id: u64,
    /// 所属字典类型 id
    pub dictionary_id: u64,
    /// 显示文本
    pub label: String,
    /// 选项值（同类型下活记录唯一）
    pub value: String,
    /// 扩展字段（JSON 字符串，业务自定义）
    pub extend: String,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
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
}

/// `sys_dictionary_detail::Model` → `DictionaryDetailResp` 字段搬运。
impl From<sys_dictionary_detail::Model> for DictionaryDetailResp {
    fn from(m: sys_dictionary_detail::Model) -> Self {
        Self {
            id: m.id,
            dictionary_id: m.dictionary_id,
            label: m.label,
            value: m.value,
            extend: m.extend,
            sort: m.sort,
            status: m.status,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充 `DictionaryDetailResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for DictionaryDetailResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 字典项列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryDetailListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 所属字典类型 id 精确过滤；不传查全部
    pub dictionary_id: Option<u64>,
    /// 模糊搜索关键字（匹配 label / value）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 停用；不传查全部
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

/// 字典项分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct DictionaryDetailFilter {
    pub dictionary_id: Option<u64>,
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 创建字典项请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDictionaryDetailReq {
    /// 所属字典类型 id（必须存在且未软删）
    pub dictionary_id: u64,
    /// 显示文本
    pub label: String,
    /// 选项值（同类型下活记录唯一）
    pub value: String,
    /// 扩展字段（JSON 字符串），可空
    pub extend: Option<String>,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
}

/// 更新字典项请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictionaryDetailReq {
    /// 目标字典项 id
    pub id: u64,
    /// 所属字典类型 id（必须存在且未软删）
    pub dictionary_id: u64,
    /// 显示文本
    pub label: String,
    /// 选项值（同类型下活记录唯一，排除自身查重）
    pub value: String,
    /// 扩展字段（JSON 字符串），可空
    pub extend: Option<String>,
    /// 排序值，越小越靠前
    pub sort: i32,
    /// 状态：`1` 启用、`0` 停用
    pub status: i8,
}
