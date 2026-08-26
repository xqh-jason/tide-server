use std::sync::Arc;

use crate::infra::config::Config;
use crate::utils::cache::Cache;

/// 应用级共享状态，注入到所有 Handler。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sea_orm::DatabaseConnection,
    /// 缓存抽象（token 黑名单 / 验证码），测试时可注入替换实现。
    pub cache: Arc<dyn Cache>,
}

impl AppState {
    pub fn new(
        config: Config,
        db: sea_orm::DatabaseConnection,
        cache: Arc<dyn Cache>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            cache,
        }
    }
}
