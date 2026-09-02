//! 数据字典 DTO（codegen 生成）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_dict;
use crate::utils::PageQuery;

/// 数据字典响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictResp {
    pub id: u64,
    pub type_code: String,
    pub label: String,
    pub value: String,
    pub sort: i32,
    pub status: i8,
    pub remark: String,
}

impl From<sys_dict::Model> for DictResp {
    fn from(m: sys_dict::Model) -> Self {
        Self {
            id: m.id,
            type_code: m.type_code,
            label: m.label,
            value: m.value,
            sort: m.sort,
            status: m.status,
            remark: m.remark,
        }
    }
}

/// 数据字典列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub type_code: Option<String>,
    pub label: Option<String>,
    pub status: Option<i8>,
}

/// 数据字典分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct DictFilter {
    pub type_code: Option<String>,
    pub label: Option<String>,
    pub status: Option<i8>,
}

/// 创建数据字典请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDictReq {
    pub type_code: String,
    pub label: String,
    pub value: String,
    pub sort: Option<i32>,
    pub status: Option<i8>,
    pub remark: Option<String>,
}

/// 更新数据字典请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDictReq {
    pub id: u64,
    pub type_code: String,
    pub label: String,
    pub value: String,
    pub sort: i32,
    pub status: i8,
    pub remark: String,
}
