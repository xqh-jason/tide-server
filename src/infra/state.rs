use std::sync::Arc;

use salvo::prelude::Depot;
use tokio_cron_scheduler::JobScheduler;

use crate::infra::config::Config;
use crate::utils::cache::Cache;
use crate::utils::error::AppError;

/// 应用级共享状态，注入到所有 Handler。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sea_orm::DatabaseConnection,
    /// 缓存抽象（token 黑名单 / 验证码），测试时可注入替换实现。
    pub cache: Arc<dyn Cache>,
    /// 定时任务调度器（W6-2）：启动时装载启用任务，CRUD 经它实时增删改。
    pub scheduler: Arc<JobScheduler>,
}

impl AppState {
    pub fn new(
        config: Config,
        db: sea_orm::DatabaseConnection,
        cache: Arc<dyn Cache>,
        scheduler: Arc<JobScheduler>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            cache,
            scheduler,
        }
    }

    /// 从请求上下文（Depot）读取注入的应用状态（`InjectState` 写入）。
    pub fn from_depot(depot: &Depot) -> Result<Self, AppError> {
        depot
            .get_typed::<AppState>()
            // 状态缺失属于内部故障（正常路径由 InjectState 保证），不是业务校验失败
            .map_err(|_| AppError::Internal(anyhow::anyhow!("app state not found")))
            .cloned()
    }
}
