//! 员工档案域：api（handler）/ service（业务）/ repo（数据访问）/ dto（传输对象）四件套。
//! 路由在此注册，由 `modules/mod.rs` 的 DOMAINS 登记表挂到 /api/v1/hr/employee 下。

use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

/// 在职状态「离职」（字典 `employmentStatus` 的 3）——**全仓唯一定义点**：
/// 离职员工不再作为审批人、不能再报加班 / 请假，额度发放范围也排除离职。
/// 别处需要这个语义时引用本常量，不要各域各写一份（值一变就静默漏判）。
pub const EMPLOYMENT_STATUS_RESIGNED: i8 = 3;

/// 员工档案端点：`POST /api/v1/hr/employee/{list,create,update,get,delete}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["员工档案"])
        .push(Router::with_path("list").post(api::list_employees))
        .push(Router::with_path("create").post(api::create_employee))
        .push(Router::with_path("update").post(api::update_employee))
        .push(Router::with_path("get").post(api::get_employee))
        .push(Router::with_path("delete").post(api::delete_employee))
}
