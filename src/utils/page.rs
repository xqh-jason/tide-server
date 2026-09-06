//! 分页通用结构：请求 `PageQuery`（JSON body 内嵌）+ 响应 `PageResult`（跨模块复用）。

use salvo::oapi::ToSchema;
use sea_orm::{DatabaseConnection, EntityTrait, FromQueryResult, PaginatorTrait, Select};
use serde::{Deserialize, Serialize};

/// 分页请求（JSON body 源，通用）。字段**只在此定义一次**：
/// 各域请求 DTO（如 `UserListReq`）通过 `#[serde(flatten)]` 内嵌本结构，
/// 由 `JsonBody<T>` 一个提取器整体反序列化；后续改字段名只改这里，
/// 所有列表接口自动生效（机制保证统一，而非靠约定）。
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    /// 页码，从 1 开始；缺省 1
    pub page: Option<u64>,
    /// 每页条数，1..=100；缺省 10
    pub page_size: Option<u64>,
}

impl PageQuery {
    /// 当前页（1-based），缺省 1
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1)
    }
    /// 页大小，缺省 10；下限 1、上限 100，防止除零与超大分页
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
    /// 转 SeaORM Paginator 用的 0-based 页号
    pub fn page_index(&self) -> u64 {
        self.page().saturating_sub(1)
    }
}

/// 分页响应：`{ total, total_pages, items }`。
/// `total` 为总条数，`total_pages` 为总页数，`items` 为当前页数据。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageResult<T> {
    /// 总条数
    pub total: u64,
    /// 总页数
    pub total_pages: u64,
    /// 当前页数据
    pub items: Vec<T>,
}

impl<T> PageResult<T> {
    pub fn new(total: u64, total_pages: u64, items: Vec<T>) -> Self {
        Self {
            total,
            total_pages,
            items,
        }
    }
}

/// repo 层分页返回：带名字的三元组，替代 `(u64, u64, Vec<T>)`。
/// 三个字段都有语义名，调用处不会再把 total 和 total_pages 弄混。
#[derive(Debug, Clone)]
pub struct PageData<T> {
    pub total: u64,       // 总条数
    pub total_pages: u64, // 总页数
    pub items: Vec<T>,    // 当前页数据
}

/// 泛型转换：只要域响应实现了 `From<Model>`（如 `RoleResp: From<sys_role::Model>`），
/// `PageData<Model>` 就能自动变成 `PageResult<域Resp>`。
/// 各域 dto 里手写的 `impl From<(u64, u64, Vec<Model>)>` 全部可以删除。
impl<T, U> From<PageData<T>> for PageResult<U>
where
    U: From<T>,
{
    fn from(data: PageData<T>) -> Self {
        PageResult::new(
            data.total,
            data.total_pages,
            data.items.into_iter().map(U::from).collect(),
        )
    }
}

/// 通用分页执行器：把「分页机械动作」收敛到一处。
///
/// 各域 repo 只负责拼过滤条件，构造出 `Select<E>` 后调用本函数，
/// 不再各自复制「num_items_and_pages + fetch_page」这一段。
///
/// - `select`：已带过滤条件的查询（`Entity::find().filter(...)` 的结果）
/// - `page_index`：0-based（由 `PageQuery::page_index()` 转换）
/// - `page_size`：1..=100（`PageQuery::page_size()` 已 clamp）
pub async fn paginate<E>(
    select: Select<E>,
    db: &DatabaseConnection,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<E::Model>>
where
    E: EntityTrait,
    E::Model: FromQueryResult + Sized + Send + Sync,
{
    // paginate / num_items_and_pages / fetch_page 来自 PaginatorTrait
    let paginator = select.paginate(db, page_size);
    let items_and_pages = paginator.num_items_and_pages().await?;
    let items = paginator.fetch_page(page_index).await?;

    Ok(PageData {
        total: items_and_pages.number_of_items,
        total_pages: items_and_pages.number_of_pages,
        items,
    })
}
