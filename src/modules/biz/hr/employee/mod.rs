//! 员工档案域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 `modules/mod.rs` 的 DOMAINS 登记表挂到 /api/v1/hr/employee 下。

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;
