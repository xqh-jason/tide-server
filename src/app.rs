use salvo::prelude::*;

use crate::config::Config;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let state = crate::state::AppState::new(config.clone());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let router = crate::router::build(state);

    tracing::info!("server listening on http://{addr}");
    let acceptor = TcpListener::new(addr).bind().await;
    Server::new(acceptor).serve(router).await;
    Ok(())
}
