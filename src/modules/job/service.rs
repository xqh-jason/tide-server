//! 定时任务业务：CRUD 与调度器实时同步（DB 为事实来源，调度器操作失败不回滚只记日志）。

use std::sync::Arc;

use sea_orm::ActiveValue::Set;
use sea_orm::ConnectionTrait;

use crate::entity::sys_job;
use crate::infra::state::AppState;
use crate::modules::job::dto::{CreateJobReq, JobFilter, JobListReq, UpdateJobReq};
use crate::modules::job::repo as job_repo;
use crate::modules::job::scheduler;
use crate::utils::PageData;
use crate::utils::datetime;
use crate::utils::error::AppError;

/// 分页查询：请求参数（keyword / status / 审计过滤）组装为 repo 过滤条件。
pub async fn page_jobs(
    db: &impl ConnectionTrait,
    req: &JobListReq,
) -> anyhow::Result<PageData<sys_job::Model>> {
    let filter = JobFilter {
        job_name: req.job_name.clone(),
        status: req.status,
        created_by: req.created_by,
        updated_by: req.updated_by,
        created_at_begin: datetime::parse_datetime("createdAtBegin", &req.created_at_begin, false)?,
        created_at_end: datetime::parse_datetime("createdAtEnd", &req.created_at_end, true)?,
        updated_at_begin: datetime::parse_datetime("updatedAtBegin", &req.updated_at_begin, false)?,
        updated_at_end: datetime::parse_datetime("updatedAtEnd", &req.updated_at_end, true)?,
    };
    job_repo::find_page(db, &filter, req.page.page_index(), req.page.page_size()).await
}

/// 查询单个详情（排除软删除）。
pub async fn get_job(db: &impl ConnectionTrait, id: u64) -> Result<sys_job::Model, AppError> {
    let Some(model) = job_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("定时任务不存在：{id}")));
    };
    Ok(model)
}

/// 创建：校验 handler 注册表 → 校验 cron → 唯一查重（含软删占位）→ 落库 →
/// 启用则注册到调度器。
///
/// 调度器操作失败不回滚 DB（重启自愈），只记 error 日志。
pub async fn create_job(
    db: &impl ConnectionTrait,
    state: &AppState,
    req: &CreateJobReq,
    actor_id: u64,
) -> Result<sys_job::Model, AppError> {
    // 1. handler 必须在内置注册表内
    scheduler::validate_handler_name(&req.handler_name)?;
    // 2. cron 表达式必须可解析
    scheduler::validate_cron_expr(&req.cron_expr)?;
    // 3. 唯一查重（含软删占位）
    if let Some(existing) = job_repo::find_by_job_name_include_deleted(db, &req.job_name).await? {
        return Err(AppError::Biz(format!(
            "任务名称已存在：{}",
            existing.job_name
        )));
    }

    // 4. 落库（审计盖章在 repo）
    let model = sys_job::ActiveModel {
        job_name: Set(req.job_name.clone()),
        cron_expr: Set(req.cron_expr.clone()),
        handler_name: Set(req.handler_name.clone()),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };
    let model = job_repo::create_job(db, model, actor_id).await?;

    // 5. 调度器接线：启用则注册（失败只记日志，不回滚 DB —— 重启自愈）
    if model.status == 1 {
        scheduler::register_job(
            state,
            model.id,
            &model.job_name,
            &model.handler_name,
            &model.cron_expr,
        )
        .await
    }
    Ok(model)
}

/// 更新：判存在 → handler/cron 校验 → 唯一查重排除自身 → 落库 → 调度器同步
/// （先 remove 旧 uuid，status=1 时用新字段重新注册）。
pub async fn update_job(
    db: &impl ConnectionTrait,
    state: &AppState,
    req: &UpdateJobReq,
    actor_id: u64,
) -> Result<sys_job::Model, AppError> {
    let Some(_) = job_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz(format!("定时任务不存在：{}", req.id)));
    };
    scheduler::validate_handler_name(&req.handler_name)?;
    scheduler::validate_cron_expr(&req.cron_expr)?;
    if let Some(existing) = job_repo::find_by_job_name_include_deleted(db, &req.job_name).await?
        && existing.id != req.id
    {
        return Err(AppError::Biz(format!(
            "任务名称已存在：{}",
            existing.job_name
        )));
    }

    let model = sys_job::ActiveModel {
        id: Set(req.id),
        job_name: Set(req.job_name.clone()),
        cron_expr: Set(req.cron_expr.clone()),
        handler_name: Set(req.handler_name.clone()),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };
    let job_model = job_repo::update_job(db, model, actor_id).await?;

    // 先移除旧调度再按需重注册：uuid 由 job_id 派生不随字段变化，cron/handler
    // 改了也必须先 remove 才能让新配置生效；禁用则只移除。
    scheduler::unregister_job(&state.scheduler, req.id).await;
    if req.status == 1 {
        scheduler::register_job(
            state,
            req.id,
            &req.job_name,
            &req.handler_name,
            &req.cron_expr,
        )
        .await;
    }
    Ok(job_model)
}

/// 删除：判存在 → 软删 → 调度器移除。
pub async fn delete_job(
    db: &impl ConnectionTrait,
    state: &AppState,
    id: u64,
) -> Result<(), AppError> {
    let Some(_) = job_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("定时任务不存在：{id}")));
    };
    job_repo::soft_delete_job(db, id).await?;

    scheduler::unregister_job(&state.scheduler, id).await;
    Ok(())
}

/// 启用 / 禁用：状态翻转 → 调度器同步（禁用 remove、启用 build + add）。
pub async fn update_job_status(
    db: &impl ConnectionTrait,
    state: &AppState,
    id: u64,
    status: i8,
    actor_id: u64,
) -> Result<sys_job::Model, AppError> {
    let Some(_) = job_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("定时任务不存在：{id}")));
    };
    job_repo::update_job_status(db, id, status, actor_id).await?;

    // 禁用移除调度；启用从 DB 取最新详情重注册（DB 是事实来源）
    let model = get_job(db, id).await?;

    if status == 0 {
        scheduler::unregister_job(&state.scheduler, id).await;
    } else {
        scheduler::register_job(
            state,
            id,
            &model.job_name,
            &model.handler_name,
            &model.cron_expr,
        )
        .await;
    }
    Ok(model)
}

/// 立即执行一次：校验任务存在后 spawn 后台执行（绕过 cron 与防重叠标志），
/// 不阻塞请求；执行结果照常写 `sys_job_log`。
pub async fn run_job_once(
    db: &impl ConnectionTrait,
    state: &AppState,
    id: u64,
) -> Result<(), AppError> {
    get_job(db, id).await?;
    scheduler::spawn_job_once(Arc::new(state.clone()), id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::job::scheduler::job_uuid;
    use crate::utils::cache::MemoryCache;
    use sea_orm::{Database, DatabaseConnection};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio_cron_scheduler::JobScheduler;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 业务 DB 走 test_txn 的回滚连接；state 只用 scheduler（测试任务 cron 均为
    /// 凌晨 3 点且调度器未 start，测试期不会真实触发）。
    async fn test_state() -> AppState {
        let config = crate::infra::config::Config::load().unwrap();
        let cache = Arc::new(MemoryCache::new());
        let scheduler = Arc::new(JobScheduler::new().await.unwrap());
        AppState::new(config, test_db().await, cache, scheduler)
    }

    fn create_req(name: String) -> CreateJobReq {
        CreateJobReq {
            job_name: name,
            cron_expr: "0 0 3 * * *".to_string(),
            handler_name: crate::task::login_log_cleanup::HANDLER_NAME.to_string(),
            status: 1,
            remark: String::new(),
        }
    }

    /// 只读探测：uuid 是否已在调度器中注册（next_tick 返回 Some 即已注册）。
    /// next_tick_for_job 需要 &mut，故克隆共享同一底层上下文的句柄来探测。
    async fn is_registered(scheduler: &JobScheduler, job_id: u64) -> bool {
        let mut probe = scheduler.clone();
        probe
            .next_tick_for_job(job_uuid(job_id))
            .await
            .unwrap()
            .is_some()
    }

    #[tokio::test]
    async fn create_job_rejects_unknown_handler() {
        let db = test_txn().await;
        let state = test_state().await;

        let mut req = create_req(unique("svc_job"));
        req.handler_name = "no_such_handler".to_string();
        let result = create_job(&db, &state, &req, 1).await;

        assert!(
            matches!(&result, Err(AppError::Biz(msg)) if msg.contains("任务处理器不存在")),
            "未知 handler 应拒绝，实际：{result:?}"
        );
    }

    #[tokio::test]
    async fn create_job_rejects_duplicate_job_name_including_soft_deleted() {
        let db = test_txn().await;
        let state = test_state().await;
        let first = create_job(&db, &state, &create_req(unique("svc_job_dup")), 1)
            .await
            .expect("首个创建只到接线 todo 处……查重应先行");

        let dup = create_job(&db, &state, &create_req(first.job_name.clone()), 1).await;

        assert!(
            matches!(&dup, Err(AppError::Biz(msg)) if msg.contains("任务名称已存在")),
            "同名创建应拒绝，实际：{dup:?}"
        );
    }

    #[tokio::test]
    async fn create_job_registers_scheduler_job_with_derived_uuid() {
        let db = test_txn().await;
        let state = test_state().await;

        let model = create_job(&db, &state, &create_req(unique("svc_job_reg")), 1)
            .await
            .expect("启用任务创建应成功并注册调度");

        assert!(
            is_registered(&state.scheduler, model.id).await,
            "启用任务应已注册到调度器（uuid 由 job_id 派生）"
        );
    }

    #[tokio::test]
    async fn delete_job_removes_scheduler_job() {
        let db = test_txn().await;
        let state = test_state().await;
        let model = create_job(&db, &state, &create_req(unique("svc_job_del")), 1)
            .await
            .expect("前置：创建启用任务");
        assert!(is_registered(&state.scheduler, model.id).await);

        delete_job(&db, &state, model.id).await.expect("删除应成功");

        assert!(
            !is_registered(&state.scheduler, model.id).await,
            "删除后调度器应已移除该任务"
        );
    }

    #[tokio::test]
    async fn update_job_status_toggles_scheduler_registration() {
        let db = test_txn().await;
        let state = test_state().await;
        let model = create_job(&db, &state, &create_req(unique("svc_job_st")), 1)
            .await
            .expect("前置：创建启用任务");
        assert!(is_registered(&state.scheduler, model.id).await);

        update_job_status(&db, &state, model.id, 0, 1)
            .await
            .expect("禁用应成功");
        assert!(
            !is_registered(&state.scheduler, model.id).await,
            "禁用后应移除调度"
        );

        update_job_status(&db, &state, model.id, 1, 1)
            .await
            .expect("重新启用应成功");
        assert!(
            is_registered(&state.scheduler, model.id).await,
            "重新启用后应恢复调度"
        );
    }
}
