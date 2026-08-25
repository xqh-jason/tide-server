use std::sync::Arc;

use crate::config::Config;

/// 应用级共享状态，注入到所有 Handler。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sea_orm::DatabaseConnection,
    // TODO(W2): pub cache: Arc<dyn Cache>（内存实现，dashmap/moka）
}

impl AppState {
    pub fn new(config: Config, db: sea_orm::DatabaseConnection) -> Self {
        Self {
            config: Arc::new(config),
            db,
        }
    }
}
