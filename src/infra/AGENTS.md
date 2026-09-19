## OVERVIEW

`src/infra/` — 启动管线、配置装载、全局状态 `AppState`、路由组装、框架级错误兜底、种子数据——业务代码只依赖这里的 `config` / `state`。

**建档理由**：得分 11（grep 实测、无 rust-analyzer：`AppState` 199 次 / 31 文件、`Config::load` 61 次 / 45 文件的高中心化 + 独立领域：全仓唯一启动路径）。

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| 启动顺序、依赖装配 | `app.rs::run` / `run_with_domains` | 前者仅内置域；后者多收 `extra` 业务域（业务仓入口） |
| 配置项、环境变量覆盖、启动期校验 | `config.rs` + 根 `config.toml` | `Config::load()` 也是全部真库测试的 DSN 来源 |
| 全局共享状态 | `state.rs::AppState` | `config` / `db` / `cache` / `scheduler` 四件（grep 实测 199 次 / 31 文件）；`from_depot` 取值 |
| 路由挂载与中间件档位 | `router.rs::build` / `build_with` | `build_with` 经 `modules::all_domains(extra)` 合并内置与业务域；`hoop_when` 挂超时豁免 |
| 框架级错误（解析失败 / 404 / 405 / 5xx） | `catcher.rs` | 统一改写成 HTTP 200 + 契约体 |
| admin / super / 菜单 / 接口权限点种子 | `seed.rs` | `MENU_SEEDS` 46 条、`API_SEEDS` 87 条（条数以代码为准，不要抄行数/条数到别的文档） |

## CONVENTIONS

- 启动顺序固定：`ConnectOptions`（`sqlx_logging` 由 `database.log_sql` 控制，默认关以免刷屏）→ 种子 → `JobScheduler::new` + **先 `init()` 再包 `Arc`** → 构造 `AppState` → `job::scheduler::init_scheduler` → 取 `config.cors` 副本建 `Cors` → `router::build`（或业务仓的 `build_with`）→ OpenAPI JSON + Swagger `unshift` → `Service::new(router).hoop(cors).catcher(...)` → `Server::serve`。
- 调度器顺序「先 add 后 start」：未 start 即 drop 会刷错误日志；`init()` 显式提前是为了 fail-fast 于装载任务之前并避开 "Uninited" 噪音。
- 种子门控：`env == "development"` 恒执行；生产仅当 `seed.enabled`（`TIDE_SEED__ENABLED=true`）执行，供首次部署 bootstrap 后立即关闭改密。`config.toml` 无 `env` 键即 development。
- 配置装载：`File::with_name("config")` + 环境变量前缀 `TIDE_`、`prefix_separator("_")`、`separator("__")`（故 `TIDE_DATABASE__URL`）、`try_parsing(true)`；`list_separator(",")` 只对 `cors.allow_origins` 生效；每字段带 `#[serde(default = ...)]`。
- `Config::load()` fail-fast 项：`jwt.ttl_seconds <= 0`、`jwt.refresh_ttl_seconds <= 0`、`upload.max_size_mb == 0`、`upload.dir` 空、非开发档位（`env` 不属于 `development` / `test`）仍用开发默认密钥。
- **`config.local.toml` 不存在**（2026-09-18 实测）：`.gitignore:10` 与 `.dockerignore:30` 都忽略了它，但 `Config::load()` 只有 `add_source(File::with_name("config"))` 一个文件源，从不读它——写了它不会报错，也不会生效。覆盖配置统一用 `TIDE_*` 环境变量（如 `TIDE_SERVER__PORT=8087`）。
- DSN 必须带 `charset=utf8mb4`（否则中文乱码）与 `timezone=%2B08:00`（否则 sqlx 强制 UTC，`CURRENT_TIMESTAMP` 差 8 小时）；容器与 CI 同时设 `TZ=Asia/Shanghai`。
- `CORS` 挂在 `Service` 层（最外）：任何来源的预检 `OPTIONS` 先被终结，不落进 catcher 与业务路由；`allow_origins` 空列表 = 拒绝所有跨源。
- 种子幂等靠「先查后插」+ `SEED_LOCK`（`OnceLock<Mutex>`）串行化，唯一索引（如 `uk_sys_menu_name`）作最后兜底。

## ANTI-PATTERNS

- 生产环境不挂载本种子初始化：`admin` 密码固定 `admin123` 且每次启动重置，会导致系统失守。
- 不让 catcher 产出真实 4xx / 5xx：它 `ctrl.skip_rest()` 后渲染契约体，HTTP 恒 200（全仓唯一的状态码例外不在本层，见 `middleware/AGENTS.md`）。
- 不在测试里无条件 `std::env::remove_var`：2026-09-13 CI 回归——抹掉 `TIDE_DATABASE__URL` 会让全量连库测试固定 30s `PoolTimedOut`；现由 `ENV_LOCK` + `EnvGuard` 保护。
- 不信任种子的「先查后插」在并发下幂等（TOCTOU）：并发路径必须靠唯一索引兜底，2026-09-11 实际踩过。
- `API_SEEDS` 是 87 条（2026-09-17 补登 `/api/v1/sys-api/list-all`，此前漏登导致该端点 fail-open）——以代码为准，改种子时同步文档。
