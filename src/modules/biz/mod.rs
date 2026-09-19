//! 业务域容器：具体业务功能域从这里生长，与平台能力（`crate::modules::system`）分离。
//!
//! 约定：
//! - 业务域按模块分组：`biz/<模块>/<域>/{api, service, repo, dto}` 四件套（按需裁剪）；
//! - 在顶层 `modules/mod.rs` 的 `DOMAINS` 登记表追加一行即可挂载，
//!   路由 URL 契约与平台域完全一致（POST + JSON body、统一响应体）；
//! - 跨域复用平台能力一律调用 `system` 内各域 service 的业务函数，
//!   不得绕过 service 直接操作他域 Entity 的业务逻辑。
//!
//! 目前：`hr`（人事）模块，子域 `employee`（员工档案）。

pub mod hr;
