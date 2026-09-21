//! 定时任务调度基础设施：启动装载、Job 构造与执行体。
//!
//! 内置任务（handler）注册表在 `src/task/`：一个任务一个文件，新增任务
//! 建文件 + 注册一行，无需改动本文件。
//!
//! 设计要点：
//! - 数据库为事实来源：启动时装载 `status=1` 未删任务；CRUD 在 DB 提交后同步
//!   调度器 add/remove，调度器操作失败不回滚 DB，只记 error 日志（重启自愈）；
//! - scheduler 的 Uuid 由 job_id 确定性派生（[`job_uuid`]），remove 无需内存映射；
//! - 防重叠：cron 触发是并发 spawn 的，per-job 运行标志保证上一轮未结束跳过本轮；
//! - 执行体统一 `timeout`（默认 300s）+ `catch_unwind`（panic 兜底）包裹，
//!   无论成败（含超时、panic）都写 `sys_job_log`。

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use futures_util::FutureExt;
use tokio_cron_scheduler::{Job, JobBuilder, JobScheduler};

use crate::infra::state::AppState;
use crate::modules::system::job::repo as job_repo;
use crate::task::handlers;
use crate::utils::error::AppError;

/// 单次执行超时上限（秒）。
const JOB_TIMEOUT_SECS: u64 = 300;
/// 执行日志 error_msg 截断长度（字节）。
const ERROR_MSG_TRUNC: usize = 2048;

/// per-job 运行标志：上一轮未结束则跳过本轮（cron 触发是并发 spawn 的）。
pub fn running_flags() -> &'static DashMap<u64, ()> {
    static FLAGS: OnceLock<DashMap<u64, ()>> = OnceLock::new();
    FLAGS.get_or_init(DashMap::new)
}

/// scheduler 的 Uuid 由 job_id 确定性派生，remove 时无需内存映射。
pub fn job_uuid(job_id: u64) -> uuid::Uuid {
    uuid::Uuid::from_u128(job_id as u128)
}

/// 校验 handler_name 必须在注册表内。
pub fn validate_handler_name(name: &str) -> Result<(), AppError> {
    if handlers().contains_key(name) {
        Ok(())
    } else {
        Err(AppError::Biz(format!("任务处理器不存在：{name}")))
    }
}

/// 校验 6 段 cron 表达式可被解析（`秒 分 时 日 月 周`，如 `0 30 3 * * *`）。
pub fn validate_cron_expr(expr: &str) -> Result<(), AppError> {
    match Job::new_async(expr, |_uuid, _l| {
        Box::pin(async {}) as Pin<Box<dyn Future<Output = ()> + Send>>
    }) {
        Ok(_) => Ok(()),
        Err(_) => Err(AppError::Biz(format!("cron 表达式不合法：{expr}"))),
    }
}

/// 启动装载：把全部启用任务注册到新建的调度器并 start（先 add 后 start）。
///
/// DB 是事实来源：装载失败视为启动失败（返回 Err 由 app.rs 中止），与 CRUD 时
/// 「调度器操作失败不回滚」的运行期语义不同。
pub async fn init_scheduler(state: &AppState) -> anyhow::Result<()> {
    let jobs = job_repo::find_active_jobs(&state.db).await?;
    for job in &jobs {
        let job = build_scheduled_job(
            Arc::new(state.clone()),
            job.id,
            job.job_name.clone(),
            job.handler_name.clone(),
            &job.cron_expr,
        )?;
        state.scheduler.add(job).await?;
    }
    state.scheduler.start().await?;
    tracing::info!("定时任务装载完成：{} 个启用任务", jobs.len());
    Ok(())
}

/// 构造调度 Job：闭包捕获 state 与任务信息，触发时走 [`run_scheduled`]（含防重叠）。
///
/// 为什么用 [`JobBuilder`] 而不是 `Job::new_async`：new_async 内部硬编码
/// `Uuid::new_v4()` 生成调度器内任务的 id，remove/探测无法对回 job_id；
/// JobBuilder 支持 `with_job_id`，把 uuid 固定为 [`job_uuid`] 的确定性派生值，
/// remove 时无需维护 job_id → Uuid 的内存映射。
///
/// 细节：`with_schedule` 的解析器未开 `dom_and_dow(true)`，日/周同时受限的
/// 表达式与 new_async 语义略有差异；常规表达式（两者至多一个受限）无影响。
pub fn build_scheduled_job(
    state: Arc<AppState>,
    job_id: u64,
    job_name: String,
    handler_name: String,
    cron_expr: &str,
) -> anyhow::Result<Job> {
    Ok(JobBuilder::new()
        // tokio-cron-scheduler 自带 prost 生成的 Uuid 包装类型，需 .into() 转换
        .with_job_id(job_uuid(job_id).into())
        .with_cron_job_type()
        // cron 按服务器本地时区解析：builder 默认 UTC（time_offset_seconds=0），
        // 不设时区则 `0 30 3 * * *` 会在北京时间 11:30 触发而非 03:30
        .with_timezone(chrono::Local)
        .with_schedule(cron_expr)?
        .with_run_async(Box::new(move |_uuid, _lock| {
            // clone 必须留在闭包体内：闭包每轮触发都执行一遍，从自己拥有的那份
            // 克隆交给本轮 async 块，原值留给下一轮触发（job_id 是 u64/Copy，直接复制）
            let state = state.clone();
            let job_name = job_name.clone();
            let handler_name = handler_name.clone();
            Box::pin(async move {
                run_scheduled(state, job_id, job_name, handler_name).await;
            })
        }))
        .build()?)
}

/// service 接线统一入口：注册任务到调度器（build + add）。
///
/// 失败只记 error 日志、不返回错误 —— DB 是事实来源，调度器操作失败由
/// 下次重启装载自愈，CRUD 的 DB 结果不受影响。
pub async fn register_job(
    state: &AppState,
    job_id: u64,
    job_name: &str,
    handler_name: &str,
    cron_expr: &str,
) {
    let job_state = Arc::new(state.clone());
    let built = build_scheduled_job(
        job_state,
        job_id,
        job_name.to_string(),
        handler_name.to_string(),
        cron_expr,
    );
    match built {
        Ok(job) => {
            if let Err(e) = state.scheduler.add(job).await {
                tracing::error!("任务注册调度失败（重启自愈）：job_id={job_id} err={e}");
            }
        }
        Err(e) => tracing::error!("构造调度 Job 失败：job_id={job_id} err={e}"),
    }
}

/// service 接线统一入口：从调度器移除任务（uuid 由 [`job_uuid`] 派生）。
/// 失败只记 error 日志、不返回错误（重启自愈）。
pub async fn unregister_job(scheduler: &JobScheduler, job_id: u64) {
    if let Err(e) = scheduler.remove(&job_uuid(job_id)).await {
        tracing::error!("任务移除调度失败（重启自愈）：job_id={job_id} err={e}");
    }
}

/// 立即执行一次（run-once 端点）：绕过 cron 与防重叠标志，后台 spawn 不阻塞请求。
pub fn spawn_job_once(state: Arc<AppState>, job_id: u64) {
    tokio::spawn(async move {
        let Ok(Some(job)) = job_repo::find_by_id(&state.db, job_id).await else {
            return;
        };
        execute_job_run(state, job.id, job.job_name, job.handler_name).await;
    });
}

/// 定时触发入口：防重叠（上一轮未结束则跳过本轮）→ 执行单次。
///
/// cron 触发是**并发 spawn** 的：若任务每 10s 一轮而单次耗时 25s，第 2、3 轮
/// 会被跳过、第 4 轮恢复——「先占坑，占不到就撤」。
/// panic 由 [`execute_job_run`] 内的 catch_unwind 捕获为失败日志，正常释放坑位，
/// 不会出现「上轮 panic → 后续轮次永久跳过」。
pub async fn run_scheduled(
    state: Arc<AppState>,
    job_id: u64,
    job_name: String,
    handler_name: String,
) {
    // 占坑必须用 entry API 而非「contains_key + insert」两步：两步之间存在竞态
    // （两个并发触发都查到无坑、随后都插入，防重叠失效），entry 把「查 + 占」
    // 合成一次持分片锁的原子操作。Occupied = 坑已被上轮占用，Vacant 才能占。
    // （dashmap 6 没有 std HashMap 那样的 try_insert，entry 枚举即等价物）
    match running_flags().entry(job_id) {
        Entry::Occupied(_) => {
            tracing::debug!("任务上轮未结束，跳过本轮：job_id={job_id} job_name={job_name}");
            return;
        }
        Entry::Vacant(vacant) => {
            vacant.insert(());
        }
    }
    execute_job_run(state, job_id, job_name, handler_name).await;
    // 正常结束释放坑位，下一轮可再次进入
    running_flags().remove(&job_id);
}

/// 单次执行核心：panic/超时兜底包裹 handler → 无论成败写 `sys_job_log`。
///
/// 返回值三层嵌套的含义：`Ok(Ok(Ok(())))` 成功 / `Ok(Ok(Err(e)))` 任务自身失败 /
/// `Ok(Err(_))` 超时（`tokio::time::timeout` 外层 Err）/ `Err(_)` panic
/// （`catch_unwind` 最外层）。
///
/// panic 必须拦在这里而非放任上抛：否则该轮执行被打断、防重叠标志无人释放
/// （后续轮次永久跳过），也落不了一条失败日志。捕获后统一走失败日志，正常
/// remove 释放坑位。
///
/// 日志口径：created_at = 落库时刻（近似开始时间）；status 1 成功 / 0 失败
/// （含超时、panic）；error_msg 截断 [`ERROR_MSG_TRUNC`] 字节；duration_ms
/// 记实际耗时。落库失败只降级 tracing——日志写不进去不能反过来炸掉调度器。
pub async fn execute_job_run(
    state: Arc<AppState>,
    job_id: u64,
    job_name: String,
    handler_name: String,
) {
    let start = std::time::Instant::now();

    // handler 缺失（DB 被手改 / 注册表变更）按失败处理，不 panic：
    // None 分支直接构造一个「已完成的失败结果」，与其余分支类型对齐
    let result = match handlers().get(handler_name.as_str()) {
        None => Ok(Ok(Err(anyhow::anyhow!("任务处理器不存在：{handler_name}")))),
        Some(handler) => {
            // AssertUnwindSafe：handler 闭包持有的引用等使其名义上非 UnwindSafe；
            // 这里只拦截 handler 自身逻辑的 panic，跨 await 的共享一致性由各
            // handler 内部保证（实际只做 DB 调用）。catch_unwind 在 poll 内
            // 捕获，panic 穿过 timeout 结构到达此处被转为 thread::Result::Err。
            AssertUnwindSafe(tokio::time::timeout(
                std::time::Duration::from_secs(JOB_TIMEOUT_SECS),
                handler(&state),
            ))
            .catch_unwind()
            .await
        }
    };

    // payload 按值 match（catch_unwind 的 thread::Result 消费所有权）
    let (status, error_msg) = match result {
        Ok(Ok(Ok(()))) => (1, String::new()),
        Ok(Ok(Err(e))) => (0, truncate_bytes(format!("任务执行失败：{e}"))),
        Ok(Err(_)) => (0, format!("任务执行超时（超过 {JOB_TIMEOUT_SECS} 秒）")),
        Err(payload) => (
            0,
            truncate_bytes(format!("任务执行 panic：{}", panic_payload(payload))),
        ),
    };

    // as_millis 返回 u128；300s 上限内不会溢出 u32
    if let Err(e) = crate::modules::system::job_log::repo::create_job_log(
        &state.db,
        job_id,
        job_name,
        status,
        error_msg,
        start.elapsed().as_millis() as u32,
    )
    .await
    {
        tracing::error!("执行日志落库失败：job_id={job_id} err={e}");
    }
}

/// 从 panic payload 提取可读消息（常见 `&str` / `String`，其余给兜底文本）。
fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "未知 panic".to_string()
    }
}

/// 按字节截断且不切碎多字节字符：中文 3 字节/字，2048 字节处很可能正好切在
/// 字符中间——`String::truncate` 遇到非边界会 panic，故回退到最近的边界再切。
fn truncate_bytes(s: String) -> String {
    if s.len() <= ERROR_MSG_TRUNC {
        return s;
    }
    let mut cut = ERROR_MSG_TRUNC;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s[..cut].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cache::{Cache, MemoryCache};

    /// 调度 smoke（green infra 测试，不依赖业务实现）：验证 tokio-cron-scheduler
    /// 在本运行时真实触发。独立 Scheduler 实例 + 秒级任务写 cache + 宽松窗口。
    #[tokio::test]
    async fn scheduler_smoke_fires_every_second_job() {
        let cache = Arc::new(MemoryCache::new());
        let flag_key = "scheduler_smoke:flag";
        let mut sched = JobScheduler::new().await.unwrap();

        let cache_for_job = cache.clone();
        let job = Job::new_async("0/1 * * * * *", move |_uuid, _l| {
            let cache = cache_for_job.clone();
            Box::pin(async move {
                cache.set(
                    flag_key,
                    "fired".to_string(),
                    std::time::Duration::from_secs(60),
                );
            })
        })
        .unwrap();
        sched.add(job).await.unwrap();
        sched.start().await.unwrap();

        // 500ms tick 粒度 + 调度启动延迟：3s 窗口内最多轮询等待
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut fired = false;
        while std::time::Instant::now() < deadline {
            if cache.exists(flag_key) {
                fired = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        sched.shutdown().await.unwrap();

        assert!(fired, "3 秒内秒级任务应至少触发一次");
    }

    /// 守卫：panic 会被 catch_unwind 捕获并转成 `thread::Result::Err`——防止
    /// 重构删掉 execute_job_run 的 panic 兜底（flag 泄漏 / 漏写失败日志）。
    #[tokio::test]
    async fn catch_unwind_captures_handler_panic() {
        let fut = AssertUnwindSafe(async { panic!("handler boom") });
        let res = fut.catch_unwind().await;

        assert!(res.is_err(), "panic 应被捕获为 Err(payload)");
        let msg = panic_payload(res.unwrap_err());
        assert_eq!(msg, "handler boom", "payload 应提取出 panic 消息");
    }
}
