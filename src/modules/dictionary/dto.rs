//! 数据字典 DTO：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::utils::PageQuery;

// —— 字典类型 ——

/// 字典类型响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryResp {
    pub id: u64,
    pub name: String,
    pub r#type: String,
    pub status: i8,
    pub remark: String,
}

impl From<sys_dictionary::Model> for DictionaryResp {
    fn from(m: sys_dictionary::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            r#type: m.r#type,
            status: m.status,
            remark: m.remark,
        }
    }
}

/// 字典类型列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    /// 对 name / type 模糊匹配
    pub keyword: Option<String>,
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
    pub name: String,
    pub r#type: String,
    pub status: i8,
    pub remark: Option<String>,
}

/// 更新字典类型请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDictionaryReq {
    pub id: u64,
    pub name: String,
    pub r#type: String,
    pub status: i8,
    pub remark: Option<String>,
}

// —— get-by-type 下拉契约 ——

/// 按类型编码取启用字典项的请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryTypeReq {
    pub r#type: String,
}

/// `get-by-type` 响应：类型信息 + 启用字典项（前端下拉一次拿全）。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryOptionResp {
    pub id: u64,
    pub name: String,
    pub r#type: String,
    pub details: Vec<DictionaryDetailOption>,
}

/// 下拉项：只带前端渲染需要的字段。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryDetailOption {
    pub id: u64,
    pub label: String,
    pub value: String,
    pub extend: String,
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
    pub id: u64,
    pub dictionary_id: u64,
    pub label: String,
    pub value: String,
    pub extend: String,
    pub sort: i32,
    pub status: i8,
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
        }
    }
}

/// 字典项列表请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryDetailListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub dictionary_id: Option<u64>,
    /// 对 label / value 模糊匹配
    pub keyword: Option<String>,
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
    pub dictionary_id: u64,
    pub label: String,
    pub value: String,
    pub extend: Option<String>,
    pub sort: i32,
    pub status: i8,
}

/// 更新字典项请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDictionaryDetailReq {
    pub id: u64,
    pub dictionary_id: u64,
    pub label: String,
    pub value: String,
    pub extend: Option<String>,
    pub sort: i32,
    pub status: i8,
}
