use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::utils::response::ApiResponse;

/// 健康检查：真实探测数据库连通性，返回统一响应体结构。
///
/// 为什么必须探活而不是回静态 `"ok"`：本项目契约是「HTTP 恒 200，成败看 code」，
/// 因此编排层的 `curl -f`（只判状态码）**永远通过** —— 数据库挂了也报健康，
/// `docker-compose.yml` 里 `frontend` 的 `depends_on: service_healthy` 会被连带误导。
/// 探活后：DB 可达 → `code:1`；不可达 → `code:0`，编排层改用 `grep '"code":1'` 判定。
///
/// 只做最轻的 `SELECT 1`：本端点属 Public 档位，不做重查询。
#[endpoint]
pub async fn health(depot: &mut Depot) -> Json<ApiResponse<String>> {
    // 状态缺失属装配错误：按不可用返回，而不是 panic
    let Ok(state) = AppState::from_depot(depot) else {
        return Json(ApiResponse::fail("服务状态未就绪"));
    };

    match probe_database(&state).await {
        Ok(()) => Json(ApiResponse::ok("ok".to_string())),
        Err(err) => {
            tracing::error!(%err, "健康检查：数据库探活失败");
            Json(ApiResponse::fail("数据库不可用"))
        }
    }
}

/// 数据库探活：执行一次最轻量的查询。
///
/// 用 sea-orm 的 `ConnectionTrait::query_one` 发 `SELECT 1`：不依赖任何业务表，
/// 因此「表被删/迁移未跑」也能如实反映连接层状态。
async fn probe_database(state: &AppState) -> anyhow::Result<()> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    state
        .db
        .query_one(Statement::from_string(
            DbBackend::MySql,
            "SELECT 1".to_string(),
        ))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::InjectState;
    use crate::utils::cache::MemoryCache;
    use salvo::test::{ResponseExt, TestClient};
    use sea_orm::{Database, DatabaseConnection};
    use std::sync::Arc;

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 构造 AppState；`db` 由调用方决定是可用连接还是不可达连接。
    async fn app_state(db: DatabaseConnection) -> AppState {
        let config = crate::infra::config::Config::load().unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        AppState::new(config, db, Arc::new(MemoryCache::new()), scheduler)
    }

    /// 与生产同构的最小服务：`InjectState`（真实注入 Depot）+ health 路由。
    fn service(state: AppState) -> Service {
        Service::new(
            Router::new()
                .hoop(InjectState(state))
                .push(Router::with_path("api/v1/health").post(health)),
        )
    }

    /// 跑一次真实 HTTP 请求，返回契约体的 (code, data, message)。
    async fn call_health(state: AppState) -> (i32, String, String) {
        let mut res = TestClient::post("http://test/api/v1/health")
            .send(&service(state))
            .await;
        let body = res.take_string().await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&body)
            .unwrap_or_else(|e| panic!("响应应为契约 JSON：{e}，实际 {body}"));
        (
            value["code"].as_i64().unwrap() as i32,
            value["data"].as_str().unwrap_or_default().to_string(),
            value["message"].as_str().unwrap_or_default().to_string(),
        )
    }

    /// 数据库可达 → code=1（happy path，探活不误报）。
    #[tokio::test]
    async fn health_returns_ok_when_database_reachable() {
        let state = app_state(test_db().await).await;

        let (code, data, message) = call_health(state).await;

        assert_eq!(
            code, 1,
            "数据库可达时健康检查应成功，实际 message={message}"
        );
        assert_eq!(data, "ok");
    }

    /// 数据库不可达 → code=0（核心回归：旧的静态 "ok" 实现会在此失败）。
    ///
    /// 仿真「进程活着但依赖挂了」：先建可用连接池，再 `close()` 令后续任何
    /// 取连接都失败——探活必须如实报失败，而不是无条件回 "ok"。
    #[tokio::test]
    async fn health_reports_failure_when_database_unreachable() {
        let db = test_db().await;
        // 关掉连接池，使后续任何取连接都失败（等价于 DB 已不可用）
        db.clone().close().await.unwrap();
        let state = app_state(db).await;

        let (code, _, message) = call_health(state).await;

        assert_eq!(
            code, 0,
            "数据库不可达时健康检查必须失败（旧的静态实现会返回 1）"
        );
        assert!(
            message.contains("数据库"),
            "失败文案应指明数据库不可用，实际：{message}"
        );
    }

    /// AppState 缺失（装配错误）→ 按不可用返回，不 panic。
    ///
    /// 不挂 `InjectState`，模拟中间件链装配错误：handler 取不到状态时必须
    /// 如实报不健康，而不是 panic 或谎报 ok。
    #[tokio::test]
    async fn health_reports_failure_when_state_missing() {
        let service =
            Service::new(Router::new().push(Router::with_path("api/v1/health").post(health)));
        let mut res = TestClient::post("http://test/api/v1/health")
            .send(&service)
            .await;
        let body = res.take_string().await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(value["code"].as_i64().unwrap(), 0, "状态缺失时不应报告健康");
    }
}
