//! 用户 DTO（传输对象）：entity（Model）不直接暴露给接口，经 From 转换脱敏。

use salvo::oapi::{ToParameters, ToSchema};
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::entity::sys_user;
use crate::utils::PageResult;

/// 用户响应体（不含密码等敏感字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResp {
    pub id: u64,
    pub username: String,
    pub nickname: String,
    pub email: String,
    pub status: i8,
}

impl From<sys_user::Model> for UserResp {
    fn from(m: sys_user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            nickname: m.nickname,
            email: m.email,
            status: m.status,
        }
    }
}

/// 用户列表过滤参数（query 源）。分页由独立的 `PageQuery` 参数提供（在 handler 里并列），
/// 这样分页字段仍统一在 utils 定义，这里只声明用户域自己的过滤条件。
/// 注：`ToParameters` derive 自带运行时提取（Extractible）+ OpenAPI 文档，
/// 且**不支持嵌套 flatten**——所以不要在这里嵌 `PageQuery`，让它作为独立 handler 参数。
#[derive(Debug, Deserialize, ToParameters)]
#[salvo(extract(default_source(from = "query")))]
pub struct UserQuery {
    pub keyword: Option<String>, // 用户名模糊搜索
    pub status: Option<i8>,
}

/// 用户域特有转换：repo 的 `(total, Vec<Model>)` → 通用分页响应。
impl From<(u64, Vec<sys_user::Model>)> for PageResult<UserResp> {
    fn from((total, items): (u64, Vec<sys_user::Model>)) -> Self {
        PageResult::new(total, items.into_iter().map(UserResp::from).collect())
    }
}
