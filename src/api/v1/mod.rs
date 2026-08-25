use salvo::prelude::*;

pub mod health;

/// v1 路由表：/api/v1 下的所有接口在此注册。
pub fn routes() -> Router {
    Router::new().push(Router::with_path("health").get(health::health))
}
