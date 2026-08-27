//! 角色域：W3 第二步实现 CRUD + 分配菜单/API（事务）。
pub mod repo;
pub mod service;

use salvo::prelude::*;

pub fn routes() -> Router {
    Router::new()
}
