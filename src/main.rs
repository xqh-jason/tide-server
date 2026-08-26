mod entity;
mod infra;
mod middleware;
mod modules;
mod task;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 日志初始化：可用 RUST_LOG 环境变量覆盖，默认 info 级别、本项目 debug。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,salvo_vben_admin=debug".into()),
        )
        .init();

    let config = infra::config::Config::load()?;
    infra::app::run(config).await
}
