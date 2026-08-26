use salvo::prelude::*;

pub mod health;

/// v1 系统级路由（健康检查等非业务接口）。
/// 业务接口在各域模块 `src/modules/<domain>/mod.rs` 的 `routes()` 注册。
pub fn routes() -> Router {
    Router::new().push(Router::with_path("health").get(health::health))
}
