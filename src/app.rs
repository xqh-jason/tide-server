use salvo::oapi::swagger_ui::SwaggerUi;
use salvo::oapi::OpenApi;
use salvo::prelude::*;

use crate::config::Config;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.server.host, config.server.port);
    // 连接 MySQL 连接池（SeaORM DatabaseConnection 内部是 sqlx 连接池，Clone 共享）。
    let db = sea_orm::Database::connect(&config.database.url).await?;
    let state = crate::state::AppState::new(config, db);

    let router = crate::router::build(state);

    // OpenAPI 契约交付：JSON 文档 + Swagger UI 页面（W1 目标）。
    // 只有 #[endpoint] 定义的接口会被 merge_router 收录。
    let doc = OpenApi::new("salvo-vben-admin API", "0.1.0").merge_router(&router);
    let router = router
        .unshift(doc.into_router("/api-doc/openapi.json"))
        .unshift(SwaggerUi::new("/api-doc/openapi.json").into_router("/swagger-ui"));

    tracing::info!("server listening on http://{addr}");
    let acceptor = TcpListener::new(addr).bind().await;
    Server::new(acceptor).serve(router).await;
    Ok(())
}
