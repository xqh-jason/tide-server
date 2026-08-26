//! 分页通用结构：请求 `PageQuery` + 响应 `PageResult`（跨模块复用）。

use salvo::oapi::{ToParameters, ToSchema};
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

/// 分页请求（query 源，通用）。字段**只在此定义一次**：
/// 各域查询结构体用 `#[salvo(extract(flatten))]` 组合它，后续改字段名只改这里，
/// 所有列表接口自动生效（机制保证统一，而非靠约定）。
/// 注：`ToParameters` derive 自带 `Extractible` 实现（运行时提取 + OpenAPI 文档），
/// 不要同时 derive `Extractible`（会冲突）。
#[derive(Debug, Deserialize, Serialize, ToSchema, ToParameters)]
#[salvo(extract(default_source(from = "query")))]
pub struct PageQuery {
    pub page: Option<u64>, // 从 1 开始
    pub page_size: Option<u64>,
}

impl PageQuery {
    /// 当前页（1-based），缺省 1
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1)
    }
    /// 页大小，缺省 10
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10)
    }
    /// 转 SeaORM Paginator 用的 0-based 页号
    pub fn page_index(&self) -> u64 {
        self.page().saturating_sub(1)
    }
}

/// 分页响应：`{ total, items }`。`total` 为总条数，`items` 为当前页数据。
#[derive(Debug, Serialize, ToSchema)]
pub struct PageResult<T> {
    pub total: u64,
    pub items: Vec<T>,
}

impl<T> PageResult<T> {
    pub fn new(total: u64, items: Vec<T>) -> Self {
        Self { total, items }
    }
}
