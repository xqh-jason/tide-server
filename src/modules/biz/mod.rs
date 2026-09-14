//! 业务域容器：具体业务功能域从这里生长，与平台能力（`crate::modules::system`）分离。
//!
//! 约定：
//! - 每个业务域一个目录 `biz/<域>/{api, service, repo, dto}` 四件套（按需裁剪）；
//! - 在顶层 `modules/mod.rs` 的 `DOMAINS` 登记表追加一行即可挂载，
//!   路由 URL 契约与平台域完全一致（POST + JSON body、统一响应体）；
//! - 跨域复用平台能力一律调用 `system` 内各域 service 的业务函数，
//!   不得绕过 service 直接操作他域 Entity 的业务逻辑。
//!
//! 目前为空：尚未有业务域落地。
