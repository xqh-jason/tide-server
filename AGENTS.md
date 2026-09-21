# Repository Guidelines

> **最高规则（优先于本文件其余全部内容）** —— 本项目是辅助学习 Rust 的练手项目，固定分工，AI 不得越界：
> - **AI 负责**：全部测试（失败测试先行 + 断言用例 + 真库夹具）、代码骨架（文件/模块结构、类型与函数签名、trait/impl 接线、`?` 错误传递、doc 注释与实现提示）、最终 code review。
> - **人负责**：具体业务实现（函数体逻辑）。AI 交付到「可编译的骨架 + 失败的测试」为止。
> - **例外（AI 可直接完成）**：配置文件、迁移 DDL、种子数据、机械登记行（`mod` 声明、`DOMAINS` 追加、`API_SEEDS` / `MENU_SEEDS` 条目）、文档。
> - **骨架体不得用 `todo!()`**（`Cargo.toml` 对 `todo` 已 `deny`）：写成可编译桩（返回 `Default` / 空集合 / `AppError::Biz("未实现")`），并用 `// 实现提示：…` 注释标出要写什么；红灯测试即交接信号。
> - 若某任务在不写实现的前提下无法交付可编译状态，先向用户确认，再动实现代码。

## Project Overview

`tide-server` 是 Rust + Salvo + SeaORM 的 RBAC 中后台后端，前端为 vue-vben-admin 基座的 `tide-admin`（同级目录）。业务按垂直切片组织、契约驱动。

- 服务监听 `0.0.0.0:8080`（`config.toml` `[server]`）。
- 默认账号 `admin / admin123`：development 档位每次启动重置（`src/infra/seed.rs`）；production 仅在 `TIDE_SEED__ENABLED=true` 时播种。

**接口契约（改动即对外破坏，须同步 README 与前端）**

- 端点一律 `POST + JSON body`；响应体 `{ code, data, message }`，`code=1` 成功 / `0` 失败；**HTTP 恒 200**。
- 唯二例外：认证失败 401（含 `/auth/refresh`）、CORS 预检 204/403。
- `/auth/refresh` 成功时返回**裸 token 字符串**（非统一信封）。
- 其它契约例外：`file/upload` 走 multipart；`file/download`、`site-config/get` 是 GET。
- 分页请求 `{ page, pageSize }`（`pageSize` clamp 1..=1000），响应 `{ total, totalPages, items }`，统一按 `id` 降序。
- 前端 vben v5 默认 `successCode=0`，`request.ts` **必须显式改成 `code === 1`**。
- OpenAPI：`/api-doc/openapi.json` + `/swagger-ui`，只收录 `#[endpoint]` 标注的 handler。

## Architecture & Data Flow

**三个独立 Cargo 包，不是 workspace**（三份 `Cargo.lock`，勿加 `[workspace]`）：

| 包 | 目录 | edition | 说明 |
|---|---|---|---|
| `tide-server` | `/` | 2024 | lib + bin 双 target；无 `[workspace]`/`[[bin]]`/`[profile]` 声明 |
| `migration` | `migrations/` | 2021 | sea-orm-migration 独立 crate，**必须在 `migrations/` 目录内执行** |
| `codegen` | `codegen/` | 2024 | JSON → entity + 四件套骨架；CI 与 Docker 均不构建 |

**启动管线**（`src/main.rs` → `src/infra/app.rs:run_with_domains`）：

```
tracing_subscriber(EnvFilter) → Config::load() → Database::connect(ConnectOptions, sqlx_logging)
→ seed::ensure_seed（dev 恒跑；prod 仅 seed.enabled）
→ JobScheduler new+init(Arc) → AppState::new → job::scheduler::init_scheduler（先 add 后 start）
→ router::build_with → OpenApi::merge_router + SwaggerUi
→ Service::new(router).hoop(Cors).catcher(catcher::build())
```

**请求链路**（中间件顺序即依赖顺序）：

```
Service → Cors（最外层，Service hoop；预检 OPTIONS 直接 204/403 + skip_rest）
  → InjectState（Depot 注入 AppState）
  → RequestTimeout（30s，hoop_when 豁免 /file/upload 与 /file/download；超时渲染契约体，不产生 4xx/5xx）
  → 每域三件套（仅 MountGuard::Protected）：AuthRequired → OperationLog → ApiPermission
  → handler（#[endpoint]）
```

**域装配**（`src/modules/mod.rs`）：`DOMAINS: &[DomainMount]`（20 行） = `path` 前缀 + `guard: MountGuard::{Public,Protected}` + `routers: &'static [fn() -> Router]`；`infra/router.rs::mount_domains` 据此循环挂载，`all_domains(extra)` 支持调用方追加盟主。`Protected` 自动获得三件套，业务域**不要**自己接鉴权（唯二自挂的是 `auth/logout` 与 `config/site-config` 的 update）。

```rust
// src/modules/mod.rs:128
DomainMount { path: "role", guard: MountGuard::Protected, routers: &[system::role::routes] }
```

**切片分层**：`api → service → repo → entity`，`utils` / `infra::{config,state}` 被各层依赖。一次业务写请求的数据流（POST `/api/v1/role/create`）：

```
role/api.rs::create_role
  → AppState::from_depot(depot)? / AuthUser::from_depot(depot)?  (typed Depot 读取)
  → dictionary::service::enabled_int_values(&db, "status")        (值域校验的唯一允许值来源)
  → role/validate.rs::validate_create_role(&req, &allowed)        (纯函数校验, Err(String))
  → role/service.rs：db.begin() → create_role_in_tx(&txn, actor_id, req) → commit   (事务边界在 service)
  → role/repo.rs：Set(actor_id) 盖审计字段 → utils::paginate      (repo 只拼 SQL)
  → Ok(ApiResponse::ok(data))  ⇒ { code: 1, data, message: "ok" }
```

## Key Directories

| 路径 | 作用 |
|---|---|
| `src/lib.rs` | 库入口：`pub mod entity / infra / middleware / modules / task / utils`（bin 从这里取模块） |
| `src/main.rs` | 进程入口：tracing + `Config::load` → `infra::app::run` |
| `src/modules/system/<域>/` | 18 个平台域切片，四件套 `api.rs` / `service.rs` / `repo.rs` / `dto.rs`（部分带 `validate.rs`、`mod.rs`） |
| `src/modules/biz/<模块>/<域>/` | 业务域容器，当前只有 `hr/employee`（**纯骨架，全部 `未实现`**，勿当参考实现） |
| `src/modules/mod.rs` | `MountGuard` / `DomainMount` / `DOMAINS` 登记表 + `all_domains` |
| `src/infra/` | 启动管线 `app.rs`、路由装配 `router.rs`、`Config`（`config.rs`）、`AppState`（`state.rs`）、全局 catcher、2052 行幂等种子 `seed.rs` |
| `src/middleware/` | `auth.rs` / `op_log.rs` / `api_permission.rs` / `request_timeout.rs` / `cors.rs` / `mod.rs`(InjectState) |
| `src/entity/` | SeaORM 实体（22 个 `DeriveEntityModel`，24 文件），全库**无物理外键**，仅 `sys_refresh_token` 声明逻辑 `Relation` |
| `src/utils/` | `response.rs`(信封) / `error.rs`(AppError) / `request.rs`(`JsonBody`) / `page.rs` / `user_ref.rs`(人字段) / `jwt.rs` / `crypt.rs` / `cache.rs` / `check.rs` / `datetime.rs` / `id_req.rs` / `text.rs` / `serde_format.rs` |
| `src/task/` | `JobHandler` 静态注册表 + 4 个保留期清理任务（天数读配置，`0` = 永久、提前返回） |
| `migrations/src/` | 20 表 baseline + 增量迁移 + 25 个 legacy 占位（保 `seaql_migrations` 版本兼容，勿删） |
| `codegen/` | `defs/*.json` 定义 → 生成 entity + api/service/repo/dto/mod 六文件 |
| `docker/`、`Dockerfile`、`docker-compose.yml` | 部署：mysql(3307→3306) + backend(8080) + frontend(80) |

新增业务域三步：`src/entity/mod.rs` 加 `pub mod <表>;` → 容器 `mod.rs`（`biz/mod.rs` 或 `system/mod.rs`）加子域声明 → `src/modules/mod.rs` 的 `DOMAINS` 加一行；**新端点必须登记 `src/infra/seed.rs` 的 `API_SEEDS`**（未登记即 fail-open 放行）。

## Development Commands

```bash
docker compose up -d mysql        # 开发 MySQL：宿主 3307 → 容器 3306，首次自动建库

cd migrations                     # 建表必须在 migrations/ 内执行
DATABASE_URL='mysql://root:root@localhost:3307/tide_server?charset=utf8mb4&timezone=%2B08:00' cargo run -- up
cd ..

cargo run                         # 启动服务；CWD 必须是仓库根（config.toml 相对路径加载）
cargo test                        # 全量真库测试；勿同时并行跑第二个 cargo test
cargo test --lib                  # 仅 lib target（bin 侧已无测试）
cargo test role                   # 单模块

cargo fmt --check && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)

# codegen（root 参数不可省，默认 .. 是相对 CWD，会写到仓库外）
cargo run --manifest-path codegen/Cargo.toml -- generate codegen/defs/job.json .
```

- **禁止主动跑** `cargo build` / `cargo build --release` / `cargo build --workspace`（仅部署 / 产物验证时执行）；日常用 `cargo check`。
- 根目录的 `cargo run -- up` 启动的是**服务**，不是迁移。
- DSN 必带 `charset=utf8mb4`（否则中文乱码）与 `timezone=%2B08:00`（否则 8 小时时间戳偏移）。

## Code Conventions & Common Patterns

**分层命名（按层统一，勿自创）**

| 层 | 分页 | CRUD / 其它 |
|---|---|---|
| repo | `find_page` | `find_by_id` / `create_*` / `update_*` / `soft_delete_*` / `find_by_*_include_deleted` |
| service | `page_<实体>` | `create_*` / `update_*` / `get_*` / `delete_*`；事务内实现 `*_in_tx`；关联表写入 `*_with_links` |
| handler | `list_<实体>` | `create_*` / `update_*` / `get_*` / `delete_*`；文件 `file::routes` 等出口带后缀（`menu::user_routes`） |

同一域内 `api.rs` 函数顺序 = `mod.rs` 路由挂载顺序 = `list → create → update → get → delete`，特殊契约端点（`info` / `access-codes` / `menus`）排在 CRUD 之后。

**错误与响应**

- repo 返回 `anyhow::Result` 并用 `?` 传播；service / api 用 `AppError`；handler 返回 `ApiResult<T> = Result<ApiResponse<T>, AppError>`。
- `AppError::Biz(String)` 面向用户，`Internal(anyhow::Error)` 对外只回固定串、内部落 `tracing::error!`。
- `From<anyhow::Error> for AppError` 是唯一收口点：MySQL 1213（死锁）/ 1205（锁等待超时）映射为 `Biz("操作冲突，请稍后重试")`，其余归 `Internal`。
- 请求体走 `JsonBody<T>`（`src/utils/request.rs`，带字段级错误定位）；成功用 `ApiResponse::ok(data)`。

**数据访问不变式**

- 软删除：主表查询必须 `.filter(Column::DeletedAt.is_null())`（无全局 scope，漏写即静默返回已删数据）；关系表（`sys_user_role` / `sys_role_menu` / `sys_role_api` / `sys_user_dept` / `sys_user_position`）硬删除；`sys_refresh_token` 无 `deleted_at`，物理清理。
- 唯一性校验走 `*_include_deleted` 变体——软删行仍占唯一键。
- 加锁读：只有「读 → 判断 → 写」不变式才 `lock_exclusive()`（`SELECT ... FOR UPDATE`，集中在 dept / menu / role / sys_api repo）；展示类查询一律普通读。
- 业务层新增函数禁止新增 `db.begin()`；多表原子性把事务边界上移到 service 入口（自持事务的 service 收 `&DatabaseConnection`）。
- 分页前统一 `order_by_desc(Id)`；过滤参数打包为域 `*Filter`（定义在 `dto.rs`），与 `page_index` / `page_size` 分离；分页机械动作统一走 `utils::paginate`。
- 审计字段 `created_by` / `updated_by` 由 repo 层盖章（`update` 只写 `updated_by`）；请求体**不接受**人字段，防伪造。人字段展示名统一走 `utils::user_ref::fill_user_names`（`动词过去式_by` → Resp `*_name`）。
- 状态值域只从 `dictionary::service::enabled_int_values` 取，不在 repo/service 里硬编码。

**边界与风格**

- repo 只拼 SQL：禁止业务保留字判断、业务错误文案、唯一性/状态机决策、跨域调用他域 repo/service。
- 超管保留字集中为常量：`SUPER_ROLE_KEY` / `ADMIN_USERNAME`（唯一定义 `modules/system/permission/mod.rs`）；按钮权限码不再有后端常量，判定面只消费接口通道（`ApiPermission`：path+method 匹配 `sys_api`，超管短路，未登记 fail-open）。
- 注释用简体中文、中英文之间留空格；标识符英文命名；中文 `//!` 模块文档写「为什么」。
- 提交信息 Conventional Commits + 中文描述，类型限 `feat` / `fix` / `refactor` / `chore` / `docs` / `test`，如 `feat(rbac): 完善软删除过滤与用户创建校验`。
- 新功能先写设计文档（`docs/superpowers/specs/`，本地 gitignored），按「测试红 → 实现 → 验证」推进。
- 文档里的硬数字（测试数、种子条数、引用计数、行号）会腐烂：改代码时同步改数字，或改写成不依赖数字的措辞。

## Important Files

- `src/lib.rs`、`src/main.rs` — 库/进程入口。
- `src/infra/app.rs` — 启动管线与 `Service`/CORS/catcher 装配。
- `src/infra/router.rs` — 路由装配、超时豁免、三件套挂钩。
- `src/infra/config.rs` — `Config::load`：`config.toml` + `TIDE_*` 覆盖 + fail-fast（production 拒绝默认密钥 `dev-secret-change-me`）。
- `src/infra/seed.rs` — 幂等种子：`SEED_ADMIN_USERNAME`、`MENU_SEEDS`、`API_SEEDS`、`SEED_LOCK`；`API_SEEDS` 是接口授权登记表。
- `src/infra/state.rs` — `AppState { config, db: DatabaseConnection, cache: Arc<dyn Cache>, scheduler }` + `from_depot`。
- `src/modules/mod.rs` — `DOMAINS` 登记表（新增域的唯一装配点）。
- `src/utils/response.rs` / `error.rs` / `request.rs` / `page.rs` / `user_ref.rs` — 跨切面唯一事实来源。
- `src/modules/system/role/*` — 完整参考切片；`src/modules/biz/hr/employee/*` — 未实现骨架模板。
- `migrations/src/lib.rs` — 迁移注册与顺序（改动即改此处 + 新增 `mYYYYMMDD_NNNNNN_*.rs`）；baseline 冻结，勿改历史。
- `codegen/defs/*.json` — 9 个真实域定义（输入格式示例）。
- `config.toml`、`.env.example`、`Dockerfile`、`docker-compose.yml`、`.github/workflows/ci.yml`、`README.md`、`CONTRIBUTING.md`。

## Runtime/Tooling Preferences

- **Rust 1.96.0**（`rust-toolchain.toml` 钉死；1.96.0 与 1.96.1 的 rustfmt 结果已分叉，勿随意升）；`profile = "minimal"` ⇒ **无 rust-analyzer**，符号检索用 `ast-grep` + `grep`（`graphify` 索引在 gitignored 的 `graphify-out/`）。
- lint：根 crate `Cargo.toml` `[lints]` 对 `unsafe_code` / `unwrap_used` / `expect_used` / `dbg_macro` / `todo` 全 `deny`；`clippy.toml` 仅放行**测试代码**的 unwrap/expect；`migrations/` 与 `codegen/` 不继承根 lint。
- 配置覆盖规则：`TIDE` + `_` + 大写 toml 路径（`.` → `__`），如 `database.url` → `TIDE_DATABASE__URL`、`log_retention.login_log_days` → `TIDE_LOG_RETENTION__LOGIN_LOG_DAYS`；**`TIDE_SEED_ENABLED`（单下划线）不是配置项**，它只是 compose 宿主机变量，容器内生效的是 `TIDE_SEED__ENABLED`。
- 数据库 MySQL 8 / utf8mb4；容器与 CI 设 `TZ=Asia/Shanghai`。
- 前端（可选）：Node `^22.18 || ^24.12` + pnpm 11，与后端同级目录，`/api` 代理 → `127.0.0.1:8080`。
- 无 `justfile` / `Makefile` / `Taskfile`；唯一脚本是 `docker/entrypoint.sh`（`RUN_MIGRATIONS=0` 可跳过迁移，否则 `./migration up` 后 `exec ./tide-server`）。
- 健康检查必须读响应体：`curl -fsS -X POST .../api/v1/health | grep -q '"code":1'`（HTTP 恒 200，`curl -f` 会永远通过）。

## Testing & QA

- 测试全部**内联**在 `src/**` 的 `#[cfg(test)] mod tests`：各域 `repo.rs` / `service.rs` / `validate.rs` + `middleware/*` + `infra/{seed,config,catcher}` + `task/*` + `utils/*`。**没有** `tests/`、`benches/`、`[[test]]` target，也**没有共享 test-util 模块**——`test_db` / `test_txn` / `unique` / `seed_*` 按模块各自复制（codegen 模板即如此生成）。
- **无 mock**：所有 DB 测试直连真 MySQL（`Config::load()` 读 `config.toml`，可被 `TIDE_DATABASE__URL` 覆盖），数据由 `ensure_seed` 播种。
- 隔离夹具：`test_txn()`（34 处定义，如 `src/modules/system/dept/service.rs:423`）= `test_db().await.begin()?`，**永不 commit**，Drop（含 panic）自动 ROLLBACK；唯一数据用 `unique(prefix)`（pid + 自增序号）。
- **sea-orm 1.1.20 真库无 savepoint**：`test_txn()` 测试里调自持事务的公共入口（`create_dept` / `delete_menu` …）会因嵌套 `begin()` 被 MySQL 隐式提交而静默失效——**只能调 `*_in_tx` 变体**。
- 需要真实提交的场景（并发 / 死锁 / 写偏斜 / 调度器 / 公共入口）改用 `test_db()` + 手工清理（`hard_delete_*` / `cleanup`），如 `dept/service.rs:468-490`、`menu/service.rs` 的 `*_via_public_entry`；7 个用例用 `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`。
- `src/infra/seed.rs` 的幂等哨兵（`ensure_seed_is_idempotent`、`ensure_seed_api_is_idempotent`）写共享种子行且**不回滚**：因此**任何时刻只跑一个 `cargo test` 进程**，全量串行。
- 规模：72 个 `#[cfg(test)]` 模块、约 550 个测试函数（543 个裸属性 + 7 个带参数）；数字随切片增长，重算：`grep -rE '^\s*#\[(tokio::)?test\]\s*$' src | wc -l`。无覆盖率门禁，断言聚焦可观测行为、边界、不变式与真实错误（`matches!(err, Err(AppError::Biz(ref m)) if m.contains("…"))`，带 `"实际 {res:?}"`）。
- CWD 敏感：`File::with_name("config")` 相对路径，必须在仓库根运行测试；改环境变量的测试需 `EnvGuard` 快照/恢复（`infra/config.rs:292-315`），且因 edition 2024 的 `env::set_var` 为 unsafe，需要模块级 `#[allow(unsafe_code)]`。
- CI（`.github/workflows/ci.yml`，`push main` / `tags: v*` / 全部 PR，`RUSTFLAGS="-D warnings"`）：`lint` job 对根 crate 与 `migrations/` 各跑一遍 fmt + clippy；`test` job `sed` 把 `config.toml` 的 `localhost:3307` 换成 `127.0.0.1:3306`、起 host-network MySQL 8、`migrate up`、先跑哨兵 `cargo test ensure_seed_api_is_idempotent -- --nocapture --test-threads=1`，再跑全量 `cargo test`。
- 提交/发版门禁：`cargo fmt --check` + `cargo clippy --all-targets -- -D warnings`（两 crate）+ `cargo test` 全绿后再打 tag。
