//! 数据字典 DTO：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::utils::PageQuery;

// —— 字典类型 ——

/// 字典类型响应体。
#[derive(Debug, Serialize, ToSchema)]
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
    /// 创建人 ID（`sys_user.id`；种子数据为 `null`）
    pub created_by: Option<u64>,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: Option<u64>,
}

impl From<sys_dictionary::Model> for DictionaryResp {
    fn from(m: sys_dictionary::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            r#type: m.r#type,
            status: m.status,
            remark: m.remark,
            created_by: m.created_by,
            updated_by: m.updated_by,
        }
    }
}

/// 字典类型列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配 name / type）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 停用；不传查全部
    pub status: Option<i8>,
}

/// 字典类型分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct DictionaryFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

/// 创建字典类型请求。
#[derive(Debug, Deserialize, ToSchema)]
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
pub struct DictionaryTypeReq {
    /// 字典类型编码（类型不存在或已停用返回业务错误）
    pub r#type: String,
}

/// `get-by-type` 响应：类型信息 + 启用字典项（前端下拉一次拿全）。
#[derive(Debug, Serialize, ToSchema)]
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
    /// 创建人 ID（`sys_user.id`；种子数据为 `null`）
    pub created_by: Option<u64>,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: Option<u64>,
}

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
            created_by: m.created_by,
            updated_by: m.updated_by,
        }
    }
}

/// 字典项列表请求。
#[derive(Debug, Deserialize, ToSchema)]
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
}

/// 字典项分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct DictionaryDetailFilter {
    pub dictionary_id: Option<u64>,
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

/// 创建字典项请求。
#[derive(Debug, Deserialize, ToSchema)]
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
