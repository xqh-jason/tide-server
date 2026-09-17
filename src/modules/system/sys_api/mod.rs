//! API 权限域（对应 sys_api 表）：权限点登记 CRUD + `sys_role_api` 关联维护。
//!
//! 授权链路只有一条：接口级集中授权（`path/method` → `sys_role_api` 角色映射，由
//! `middleware/api_permission.rs` 消费，未登记接口放行）。按钮权限码
//! （`sys_menu.permission`）不下发到判定面，只经 `/access-codes` 控前端按钮显隐
//! （原 service 层按钮码校验已于 2026-09-17 连根删除）。
//!
//! # 授权管理权的语义（2026-09-17 明确，勿再按漏洞提报）
//!
//! 本域三个写入端点（`sys-api/{create,update,delete}`）**不做按钮权限码校验**，
//! 过接口级授权即可管理登记。这是刻意的全权委托：授权管理员可以把任意接口授予
//! 任意角色（含自己持有的角色），并据此取得 `role/update` 等接口 → 实际等价于
//! 授权层的超管。因此**不设**「不得为自己所在角色授权」这类限制——能改授权数据的
//! 人本就是授权管理员，再限制他只是一种自相矛盾的规则。委托 `sys-api/*` 授权时，
//! 应按「等同超管的授权管理权」评估风险。
//!
//! 仅剩两条硬边界由 `permission` 域的保留字保证：`SUPER_ROLE_KEY`（普通角色不得
//! 创建/改名占用 `super`）与 `ensure_no_super_assignment`（非 super 操作者不得把
//! `super` 角色分配出去）。
//!
//! 另需注意：`status=0`（禁用）与软删都会让该接口回到「未登记 → 放行」状态
//! （判定见 `permission/service.rs` 的 `has_api_permission`），即摘掉一条登记
//! 等于对所有人放开该接口；且软删在 API 侧无法复原（`create_api` 被含软删查重挡住、
//! 种子明确不复活），禁用则可以改回 `status=1` 恢复。

use salvo::Router;
use salvo::oapi::RouterExt;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;
mod validate;

/// API 权限点端点：`POST /api/v1/sys-api/{list,create,update,get,delete,list-all}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["接口权限"])
        .push(Router::with_path("list").post(api::list_apis))
        .push(Router::with_path("create").post(api::create_api))
        .push(Router::with_path("update").post(api::update_api))
        .push(Router::with_path("get").post(api::get_api))
        .push(Router::with_path("delete").post(api::delete_api))
        .push(Router::with_path("list-all").post(api::list_all_apis))
}
