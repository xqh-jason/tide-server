# W6-2 定时任务模块设计规格（sys_job / sys_job_log）

> 版本：v1 ｜ 日期：2026-09-07 ｜ 状态：已定稿
> 语义对齐 gin-vue-admin `sys_job` / `sys_job_log`，字段与端点按本项目规范重设计。

---

## 1. 背景与学习目标

W6 第二个扩展模块：周期任务调度。学习目标：

- tokio-cron-scheduler 异步调度与 Job 生命周期管理；
- CRUD ↔ 调度器实时同步（数据库为事实来源）；
- `AppState` 扩展（`Arc<JobScheduler>` 共享）与启动管线集成；
- codegen 第二次主链路产出（job 四件套生成 + 裁剪接线）。

## 2. 选型与关键事实（已核实）

- **tokio-cron-scheduler 0.15.1**：`JobScheduler::new().await` → `add_job(Job)` 返回 Uuid → `start().await`；`remove_job(Uuid)` 移除；内部 **500ms tick** 粒度（触发时刻最多偏差 500ms）。
- **cron 表达式为 6 段秒级**：`秒 分 时 日 月 周`（如 `0 30 3 * * *` = 每日 03:30:00）。表格存 6 段原文，合法性由 crate 解析校验；管理界面提示格式。
- 依赖：uuid 1 / dashmap 6 / tokio 1.44 已在项目内；仅新增 tokio-cron-scheduler。
- rsproxy 镜像可用性在骨架第一步验证（salvo_extra 先例：缺包则换源/vendor）。

## 3. 数据模型

### sys_job（任务定义）

| 字段 | 类型 | 说明 |
|---|---|---|
| id | BIGINT UNSIGNED PK | 自增 |
| job_name | VARCHAR(64) | 显示名，**唯一含软删占位** |
| cron_expr | VARCHAR(64) | 6 段秒级表达式原文 |
| handler_name | VARCHAR(64) | 内置注册表键，未知值校验拒绝 |
| status | TINYINT | 1 启用 / 0 停用 |
| remark | VARCHAR(255) | 备注 |
| created_by / updated_by | BIGINT UNSIGNED | 审计（repo 盖章，规范同 AGENTS.md） |
| created_at / updated_at / deleted_at | DATETIME | 软删主表 |

### sys_job_log（执行日志）

| 字段 | 类型 | 说明 |
|---|---|---|
| id | BIGINT UNSIGNED PK | 自增 |
| job_id | BIGINT UNSIGNED | 指向 sys_job.id，不设外键；主任务删除后日志保留 |
| job_name | VARCHAR(64) | 冗余存，主任务删除后仍可读 |
| status | TINYINT | 1 成功 / 0 失败（含超时、panic） |
| error_msg | TEXT | 失败原因，截断 2KB |
| duration_ms | INT UNSIGNED | 本次耗时 |
| created_at | DATETIME | 即本次开始时间 |
| deleted_at | DATETIME NULL | 跟随现有日志表设计 |

索引：`idx_sys_job_log_job_id_created_at (job_id, created_at DESC)`；sys_job 补 `uk_job_name`（唯一）。

## 4. 调度器生命周期（核心设计）

1. **启动装载**（app.rs → `job::scheduler::init_scheduler(&state)`）：
   `JobScheduler::new()` → 调 `job_repo::find_active_jobs(db)`（status=1 未删）→ 逐个
   `add_job(构造闭包)` → `start()` → 存入 `AppState.scheduler: Arc<JobScheduler>`。
   顺序固定**先 add 后 start**（未 start 即 drop 会刷错误日志）。
2. **job ↔ scheduler 映射**：scheduler 的 Uuid 由 `Uuid::from_u128(job_id as u128)` 确定性派生，
   remove 时无需内存映射表。
3. **CRUD 实时生效**（service 层，事务提交后同步调度器）：
   - create：入库 → status=1 则 add_job；
   - update：cron/handler 变更 → 先 remove 旧 Uuid 再按新 status 决定 add；
   - update-status：0 → remove；1 → add；
   - delete：remove → 软删（同一事务 + 调度器操作在事务成功后执行）。
   - **调度器操作失败不回滚数据库**（DB 是事实来源，重启自愈），只记 error 日志。
4. **执行体 `job_runner(state, job_id, job_name, handler_name)`**：
   - 防重叠：`DashMap<u64, ()>` per-job 运行标志，`try_insert` 失败 → 记日志跳过本轮；
   - `tokio::time::timeout(Duration::from_secs(300), handler(state))` 包裹；
   - 无论成败写 `sys_job_log`（duration_ms / error 截断 2KB），落库失败降级 tracing；
   - panic 按 tokio spawn 语义捕获为失败日志（JoinHandle 判定）。

## 5. 内置 handler 注册表（位于 `src/task/`，一个任务一个文件）

```rust
// src/task/mod.rs
type JobHandler = fn(&AppState) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>;
pub fn handlers() -> &'static HashMap<&'static str, JobHandler>;
// src/task/login_log_cleanup.rs / job_log_cleanup.rs —— 文件名 = handler_name
```

v1 内置两个（async fn 装箱为 BoxFuture）：

- `cleanup_login_logs`：物理删除 90 天前的 `sys_login_log`；
- `cleanup_job_logs`：物理删除 180 天前的 `sys_job_log`（自举：自身日志自身清）。

CRUD 时校验 `handler_name` 必须在注册表内，未知值报 `任务处理器不存在：{name}`。
新增任务的固定动作：建 `src/task/<name>.rs` 实现 `run` + `task/mod.rs` 注册一行，
不碰 scheduler；任务跨域只调用各域 repo 函数（如 `login_log::repo::delete_created_before`），
不直接操作其他域 Entity。

## 6. API 契约（统一 POST + JSON）

- `/api/v1/job/{list,create,update,get,delete,update-status,run-once}`：
  - run-once：`{id}`，立即 spawn 一次执行体（绕过 cron 与防重叠标志），用于测试与运维；
  - 其余语义与既有 CRUD 域一致；审计字段 repo 盖章。
- `/api/v1/job-log/{list,get,delete,delete-batch}`：对齐 login_log 只读模式
  （job_id 精确 + status 精确 + created_at 倒序）。
- 挂载：`AuthRequired → OperationLog → ApiPermission` 三件套（与其他域一致）。
- 菜单与权限码：页面「定时任务」+ `system:job:{create,update,delete,update-status,run-once}`、
  `system:job-log:delete`；种子含一个启用示例任务（登录日志清理，每日 `0 30 3 * * *`）。

## 7. 测试策略

- **repo/service**：`test_txn` 事务回滚风格；唯一查重含软删、update-status、未知 handler 拒绝、
  分页审计过滤（过滤块在 keyword 之外——遵守 09-07 修复后的模式）。
- **启动装载**：`find_active_jobs` 纯查询测试（只含启用未删行）。
- **调度真触发 smoke**（green infra 测试，不依赖用户代码）：独立 Scheduler + 每秒任务
  （`0/1 * * * * *`）向 MemoryCache 写标志 → 2.5s 窗口断言 → shutdown；宽松窗口防时序 flaky。
- **红线**：service CRUD 接线测试在用户实现前为红灯（断言调度器 add/remove 行为）。

## 8. 风险与规避

| 风险 | 规避 |
|---|---|
| rsproxy 缺 tokio-cron-scheduler | 换源或 vendor（salvo_extra 先例），骨架第一步验证 |
| 500ms tick 粒度 | smoke 断言窗口放宽到 2.5s |
| 时序测试 flaky | 独立 Scheduler 实例 + 用例内重试读取 + 显式 shutdown |
| JobScheduler 未 start 即 drop | 启动顺序先 add 后 start；测试用例显式 shutdown |
| 实现 todo!() 期间 cargo run 启动装载 panic | 属预期红线状态；先补 `find_active_jobs` 即可启动 |
