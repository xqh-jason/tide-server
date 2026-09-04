//! 通用按 id 查询 / 删除请求体：所有域共用同构的 `{ id: u64 }` 结构。

use salvo::oapi::ToSchema;
use serde::Deserialize;

/// 通用 id 请求体（JSON body：`{ "id": ... }`）。
///
/// 各域的 `get_*` / `delete_*` 端点（用户、角色、菜单、API、字典等）统一复用本结构，
/// 避免为每个域重复定义同构的 `XxxIdReq`；后续改字段名只改这里。
#[derive(Debug, Deserialize, ToSchema)]
pub struct IdReq {
    /// 目标记录主键 id
    pub id: u64,
}
