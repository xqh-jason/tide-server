mod entity;
mod infra;
mod middleware;
mod modules;
mod task;
mod utils;

/// 日志时间戳：本地时间到秒。默认 fmt timer 输出 UTC RFC3339
/// （如 2026-09-07T07:07:14.897536Z），可读性差，这里统一替换。
#[derive(Clone, Default)]
struct LocalSeconds;

impl tracing_subscriber::fmt::time::FormatTime for LocalSeconds {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 日志初始化：可用 RUST_LOG 环境变量覆盖，默认 info 级别、本项目 debug。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tide_server=debug".into()),
        )
        .with_timer(LocalSeconds)
        .init();

    let config = infra::config::Config::load()?;
    infra::app::run(config).await
}
