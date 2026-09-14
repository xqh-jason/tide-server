use salvo::oapi::OpenApi;
use salvo::oapi::swagger_ui::SwaggerUi;
use salvo::prelude::*;

use crate::infra::config::Config;
use crate::utils::cache::MemoryCache;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.server.host, config.server.port);
    // 显式用 ConnectOptions：关掉 sea-orm 默认开启的 SQL 语句日志，避免控制台刷屏。
    let mut opt = sea_orm::ConnectOptions::new(config.database.url.clone());
    opt.sqlx_logging(config.database.log_sql);
    let db = sea_orm::Database::connect(opt).await?;
    // 种子数据初始化（幂等）：admin / super / 默认菜单与 RBAC 关联。
    // development 环境恒执行；生产仅当显式开启 seed.enabled（TIDE_SEED__ENABLED=true）
    // 时执行——供首次部署一次性 bootstrap 初始账号，完成后应立即关闭并改密。
    // 不置位时跳过，避免每次启动把 admin 密码重置为弱口令导致系统失守。
    if config.env == "development" || config.seed.enabled {
        crate::infra::seed::ensure_seed(&db).await?;
    } else {
        tracing::info!("env={} 且未开启 seed，跳过种子数据初始化", config.env);
    }
    // 定时任务调度器：先建调度器与 AppState，再装载启用任务并 start。
    // 顺序固定「先 add 后 start」——未 start 即 drop 会刷错误日志。
    // 包 Arc 前显式 init：add/start 内部虽有惰性 init（幂等），但首次 add 会打印
    // 噪音日志 "Uninited"，且 init 失败应尽早 fail-fast 于装载任务之前。
    let mut scheduler = tokio_cron_scheduler::JobScheduler::new().await?;
    scheduler.init().await?;
    let scheduler = std::sync::Arc::new(scheduler);
    let state = crate::infra::state::AppState::new(
        config,
        db,
        std::sync::Arc::new(MemoryCache::new()),
        scheduler,
    );
    crate::modules::system::job::scheduler::init_scheduler(&state).await?;

    // CORS 中间件挂在 Service 层（先于 router 的 InjectState），因此提前取出配置副本构造，
    // 不依赖 Depot 注入的状态。
    let cors = crate::middleware::cors::Cors(state.config.cors.clone());
    let router = crate::infra::router::build(state);

    // OpenAPI 契约交付：JSON 文档 + Swagger UI 页面。
    // 只有 #[endpoint] 定义的接口会被 merge_router 收录。
    let doc = OpenApi::new("tide-server API", "0.1.0").merge_router(&router);
    let router = router
        .unshift(doc.into_router("/api-doc/openapi.json"))
        .unshift(SwaggerUi::new("/api-doc/openapi.json").into_router("/swagger-ui"));

    tracing::info!("server listening on http://{addr}");
    let acceptor = TcpListener::new(addr).bind().await;
    // 统一错误兜底：框架级错误（解析失败/404/405/5xx）渲染为 HTTP 200 + 契约体。
    // CORS 防御挂在最外层：任何来源的预检请求（OPTIONS）都会先被 CORS 中间件终结，
    // 不落入 catcher / 业务路由。
    let service = salvo::Service::new(router)
        .hoop(cors)
        .catcher(crate::infra::catcher::build());
    Server::new(acceptor).serve(service).await;
    Ok(())
}
