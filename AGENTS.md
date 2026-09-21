# Repository Guidelines

> 本文件是仓库**唯一**的知识文档。此前引用的各目录 `AGENTS.md`（`src/AGENTS.md`、`src/modules/system/AGENTS.md`、
> `codegen/AGENTS.md`、`migrations/AGENTS.md` …）**在磁盘上不存在**，不要再去找、也不要再引用。
> 约定变更请直接改本文件；本文件自身被 `.git/info/exclude` 排除（本地文件，不进提交）。
> 文档里的硬数字（测试条数、种子条数、行号）随切片增长会腐烂：改代码时同步改数字，或改用不依赖数字的措辞。

## Project Overview

Rust + Salvo + SeaORM 的 **RBAC 中后台后端**，配套前端 [tide-admin](https://github.com/xqh-jason/tide-admin)
（Vue Vben Admin 5.x 基座，同级仓库，不在本仓）。定位：辅助学习 Rust 的练手项目 + 生产形态代码。

技术栈：Salvo 0.95（仅 `oapi`）/ SeaORM 1.x（`sqlx-mysql` + `runtime-tokio`）/ MySQL 8（utf8mb4）/
jsonwebtoken 双凭证 JWT / tokio-cron-scheduler / config-rs。部署走 Docker 多阶段构建 + compose + GitHub Actions。

**接口契约（全仓唯一信封）**：端点一律 `POST + JSON body`；响应体 `{ code, data, message }`
（`code=1` 成功 / `0` 失败），**HTTP 恒 200**。例外只有三个：认证失败真 401（含 `/auth/refresh`）、
CORS 预检 204/403、`file/upload` 走 multipart + `file/download` 与 `site-config/get` 是 GET。
`/auth/refresh` 成功时响应体是**裸 token 字符串**（非信封）。vben v5 默认 `successCode=0`，前端必须显式改成 `code === 1`。

### 最高规则：AI 与作者的分工（优先于本文件其余全部内容）

- **AI 负责**：所有测试（失败测试先行 + 断言用例 + 真库夹具）、代码骨架（文件/模块结构、类型与函数签名、
  trait/impl 接线、`?` 错误传递、doc 注释与实现提示）、最终 code review。
- **人负责**：具体业务实现（函数体逻辑）。AI 交付到「**可编译的骨架 + 失败的测试**」为止。
- **例外（可直接完成）**：配置文件、迁移 DDL、种子数据、机械登记行（`mod` 声明、`DOMAINS` 追加、
  `API_SEEDS` / `MENU_SEEDS` 条目）、文档。
- **骨架体禁止 `todo!()`**（`Cargo.toml` 对 `todo` 已 `deny`）：写成可编译桩 —— 返回 `Default` / 空集合 /
  `Err(AppError::Biz("未实现：<函数名>".into()))`，未用参数用 `let _ = (a, b);` 消音（CI 是 `-D warnings`），
  并加 `// 实现提示：…` 注释（不写答案代码）。**测试保持红灯 = 交接信号。**
- 新功能先建设计文档：`docs/superpowers/specs/YYYY-MM-DD-<topic>.md`（本地目录，`docs/` 被 gitignore），
  按「测试红 → 实现 → 验证」推进。

## Architecture & Data Flow

### 包拓扑
**不是 workspace**，是三个彼此独立的 crate（各自 `Cargo.lock`，根命令天然碰不到后两个）：

| 包 | 路径 | edition | 作用 |
|---|---|---|---|
| `tide-server` | `Cargo.toml` | 2024 | 应用（lib + bin 双 target） |
| `migration` | `migrations/` | 2021 | sea-orm-migration，产出二进制 `migration`（`./migration up`） |
| `codegen` | `codegen/` | 2024 | def JSON → entity + 四件套骨架 |

### lib + bin 双 target
`src/lib.rs` 整体公开 6 个模块（`entity` / `infra` / `middleware` / `modules` / `task` / `utils`），
业务全在库里；`src/main.rs` 只做进程级的事：tracing 初始化（默认 `info,tide_server=debug`）→
`Config::load()` → `infra::app::run`。**`main.rs` 不解析 argv** —— 所以根目录 `cargo run -- up` 是启动服务而不是迁移。

### 启动管线（`src/infra/app.rs`）
1. `ConnectOptions::new(config.database.url)`（`sqlx_logging` 由 `database.log_sql` 决定）→ `Database::connect`
2. 条件播种：`config.env == "development" || config.seed.enabled` → `infra::seed::ensure_seed`（幂等 + 进程级 `SEED_LOCK`）
3. `JobScheduler::new()` + 显式 `init()`（在 `Arc` 之前，失败即 fail-fast）
4. `AppState { config, db, cache, scheduler }` → `job::scheduler::init_scheduler`
5. `Cors` hoop（挂在 `Service` 上，不依赖 Depot）→ `router::build_with` → OpenAPI/SwaggerUi 前置挂载
   （`/api-doc/openapi.json`、`/swagger-ui`，只收 `#[endpoint]`）→ `TcpListener` bind → `catcher::build()`

### 请求管线（顺序即语义）
`Cors`（含预检，Service 级）→ `InjectState` → `RequestTimeout` 30s（`hoop_when(..., !is_exempt(req))`）
→ 域路由 → **Protected 出口才有**：`AuthRequired` → `OperationLog` → `ApiPermission` → handler。
- `AuthRequired`：`Bearer` → `utils::jwt::verify` → 单查询校验用户未软删且 `status == 1` → `AuthUser` 注入 Depot；
  失败 **HTTP 401** + `未登录或登录已过期` + `skip_rest()`。
- `OperationLog`：只记 POST，且跳过只读后缀（`/list` `/get` `/info` `/menus` `/access-codes` `/list-all`
  `/list-all-includes-soft-deleted` `/by-username` `/get-depts` `/get-positions` `/get-by-type` `/handlers` `/download`）；
  请求体递归脱敏（password / token / authorization / secret）并截断 4096 字节。
- `ApiPermission`：`permission::service::has_api_permission(&db, user_id, path, method)`；路径先经
  `canonical_path` 归一（百分号解码、丢空段）再查 `sys_api`。**未登记的 `path + method` 一律放行（fail-open）**；
  拒绝是 HTTP 200 + `code:0` + `无该接口访问权限`（不是 403）。
- `RequestTimeout`：30s，超时 `code:0` + `请求处理超时，请稍后重试` + `Connection: close`；
  仅 `/file/upload` 与 `/file/download` 豁免（`request_timeout::is_exempt`）。

### 分层与事务
- 方向固定：`api → service → repo → entity`；`utils`、`infra::{config,state}` 被各层依赖。
- 例外：`middleware/*`、`task/*`、`infra::seed`、`modules/system/job/scheduler.rs` 可直调目标域 **repo**。
- `api.rs` 只做 `validate_*` → `service::*` → `fill_user_names` → `Ok(ApiResponse::ok(...))`，不碰 repo / sea_orm。
- **repo 只拼 SQL**：不做业务保留字判断、不写业务错误文案、不做唯一性 / 状态机决策、不跨域调用他域 repo/service
  （跨域校验放 service）。校验分家：纯函数规则进各域私有 `validate.rs`（`mod validate;`，**刻意不 `pub mod`**），
  需要查库的规则（唯一性、存在性、超管冻结）留在 service。
- `db.begin()` **只出现在自持事务边界的 service 入口**（role / user / sys_api / menu / dept / position / dictionary 的
  create·update·delete 等）；这些入口收 `db: &DatabaseConnection`，内部委托 `pub(crate) ..._in_tx(txn: &DatabaseTransaction, ...)`。
  sea-orm 1.1.20 + MySQL **无 savepoint**，嵌套 `begin()` 会被隐式提交 —— 需要多表原子性就把事务边界上移到 service。
- 「读 → 判断 → 写」不变式才用 `.lock_exclusive()`（`SELECT ... FOR UPDATE`，只在 `*_in_tx` / 加锁读 helper 内）；
  展示类查询（列表 / 详情 / 名称拼装）**一律普通读**。

### 路由装配（`DOMAINS` 登记表）
`src/modules/mod.rs` 定义 `MountGuard { Public, Protected }`、`DomainMount { path, guard, routers }`、
`const DOMAINS: &[DomainMount]`（内置 19 行）与 `all_domains(extra)`（内置在前、extra 追加）。
`src/infra/router.rs` 的 `mount_domains` 按表循环：`path` 为 `api/v1` 下的前缀，空串表示出口自带路径；
`Protected` 行自动获得 `AuthRequired + OperationLog + ApiPermission` 三件套，**不用自己接鉴权**。
`build(state)` = `build_with(state, &[])`；自定义域集合走 `infra::app::run_with_domains(config, EXTRA)`。

### 分支纪律与硬约束（`main` / `hr`）

平台分支 `main` 与业务分支 `hr`（人事系统，见各业务分支自己的文档）之间**只允许 `main → hr` 单向合并**。
GitHub 的分支保护只能按 base 分支与状态检查过滤、没有「按源分支过滤」的规则，因此这条纪律由三处硬约束合成：

| 层 | 位置 | 作用 |
|---|---|---|
| 分支保护 | `main`（`enforce_admins: true`、require PR、required checks `fmt + clippy` / `真库集成测试` / `禁 hr→main 合并`、禁 force push / 禁删除） | 挡直接 push 与 force push |
| CODEOWNERS | `.github/CODEOWNERS`（`* @xqh-jason`） | 任何进 `main` 的 PR 都落到 owner 名下 |
| CI job | `ci.yml` 的 `guard-merge-direction`（job 名 `禁 hr→main 合并`） | `head=hr` 且 `base=main` 的 PR 直接失败 |

注意三点：owner 必须写 `@用户名`（裸名被当邮箱解析 = 没有 owner）；`guard-merge-direction` 刻意
`if: always()` 让 check 名在每次 CI 出现，否则挂不上 required checks；`main` 受保护后**平台修复也要走
分支 → PR → CI 绿 → 合并**（`enforce_admins: true` 下管理员无法直接 push），`hr` 分支保持可直推。

## Key Directories

```
src/lib.rs, src/main.rs        # 库入口（6 pub mod）/ 进程入口（tracing + Config::load + run）
src/modules/mod.rs             # MountGuard / DomainMount / DOMAINS(19) / all_domains
src/modules/system/<域>/       # 18 个平台域切片：api/service/repo/dto(+validate)
src/modules/biz/<模块>/<域>/    # 业务域容器（当前仅 mod.rs，业务从这里生长）
src/infra/                     # app 启动管线、config、state、router 装配、catcher、seed
src/middleware/                # InjectState / AuthRequired / OperationLog / ApiPermission / RequestTimeout / Cors
src/entity/                    # 21 张表的 SeaORM 实体（全局共享，含跨域关系表）+ prelude；全库无物理外键
src/utils/                     # error、response、request、page、user_ref、jwt、crypt、cache、check、datetime、text、serde_format
src/task/                      # 定时任务注册表 + 4 个保留期清理任务
migrations/                    # 独立 crate：baseline + 追加迁移（DDL 事实来源）
codegen/                       # 独立 crate：defs/*.json → entity + 四件套骨架
docker/, Dockerfile, docker-compose.yml, .github/workflows/ci.yml
docs/superpowers/{specs,plans}/  # 本地设计文档 / 计划（gitignored，非事实来源）
graphify-out/                  # 可再生的本地 AST 索引（gitignored，勿当文档读）
```

18 个平台域：`auth` `captcha` `config` `dept` `dictionary` `file` `health` `job` `job_log` `login_log`
`menu` `operation_log` `permission` `position` `refresh_token` `role` `sys_api` `user`
（`permission` 只做鉴权判定，无 `api.rs`/`dto.rs`；`health` 只有 `api.rs`）。

## Development Commands

```bash
# 1) 起开发库（宿主 3307 → 容器 3306，首次自动建库）
docker compose up -d mysql

# 2) 建表 —— 迁移是独立 crate，必须在 migrations/ 里执行
cd migrations
DATABASE_URL='mysql://root:root@localhost:3307/tide_server?charset=utf8mb4&timezone=%2B08:00' cargo run -- up
cd ..

# 3) 起服务：0.0.0.0:8080，development 档位自动跑幂等种子（admin/admin123，每次启动重置）
cargo run
RUST_LOG=tide_server=debug cargo run        # 默认过滤器已含 tide_server=debug

# 4) 校验三连（CI 同款；两个 crate 各跑一次）
cargo fmt --check      && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)
cargo test

# 5) 测试（需本地 MySQL；cargo test --lib 只跑库侧，cargo test role 只跑单模块）
cargo test
cargo test --lib
cargo test ensure_seed_api_is_idempotent -- --nocapture --test-threads=1   # CI 哨兵

# 6) codegen（必须 cd 进去：root 缺省是相对 CWD 的 ..，在仓库根跑会写进父目录）
cd codegen && cargo run -- generate defs/config.json

# 7) 代码检索（无 rust-analyzer）
graphify update .        # AST 增量刷新，零 API 费用
graphify query / graphify path / graphify explain
```

**陷阱与禁令**
- **禁止主动跑 `cargo build` / `--release` / `--workspace`**（仅部署 / 产物验证时才允许）；日常用 `cargo check` / `cargo test`。
- 根目录 `cargo run -- up` **启动服务**而非迁移（`main.rs` 不读 argv）。
- **绝不并行跑两个 `cargo test`**：多域测试共享种子行（admin、super 角色、`sys_menu` 名称、`API_SEEDS` 行），互相干扰。
- `cargo test` 同时跑 lib 与 bin 两个 target；只要库侧测试时加 `--lib`。
- 健康检查必须读响应体：`curl -X POST http://localhost:8080/api/v1/health | grep '"code":1'`（HTTP 恒 200，`curl -f` 无效）。
- 发版：门禁全绿后再 `git tag vX.Y.Z`（CI 监听 `tags: ["v*"]`，但 CI 是事后发现）。

## Code Conventions & Common Patterns

### 命名与顺序
- repo：`find_page` / `find_by_id` / `create_*` / `update_*` / `soft_delete_*` / `find_by_*_include_deleted`；
  service：`page_<实体>` / `create_<实体>` / `update_<实体>` / `get_<实体>` / `delete_<实体>`；
  handler：`list_<实体>` / `create_<实体>` / …。
- 写关系表的函数后缀 `_with_links`（`create_role_with_links`）；事务内实现后缀 `_in_tx`。
- **`api.rs` 函数顺序 = `mod.rs` 路由挂载顺序 = `list → create → update → get → delete`**，特殊/契约端点
  （`info` `access-codes` `menus` `get-by-type` `list-all*` `update-status` `handlers` `run-once`）追加在后。
  file 域是 `list → upload → get → download → delete`（先例，照抄）。
- 可选过滤条件收进每域的 `*Filter` 结构（`RoleFilter` / `UserFilter` / `MenuFilter`，声明在 `dto.rs`），
  与 `page_index` / `page_size` 分开；加过滤条件只改 Filter，不动 repo 签名。
- DTO 一律 `#[serde(rename_all = "camelCase")]`；`IdReq`（`{id}`）复用于所有 get/delete；
  时间戳走 `utils::serde_format::naive_datetime`。
- 注释用**简体中文**、中英文之间留空格；标识符用英文。

### 错误与响应
- 唯一业务错误 `AppError`（`src/utils/error.rs`）：`Biz(String)` | `Internal(#[source] anyhow::Error)`。
  `From<anyhow::Error>` 识别 MySQL **1213（死锁）/ 1205（锁等待超时）** → `AppError::Biz("操作冲突，请稍后重试")`；
  `Internal` 记日志后对外固定返回 `"internal error"`。
- handler 返回 `ApiResult<T>`（= `Result<ApiResponse<T>, AppError>`），repo 层 `?` 传播，不吞错。
- 入参统一 `JsonBody<T>` 提取器（要求 `application/json`、拒空体、把原始 JSON 存 Depot 供操作日志用），
  用 `body.into_inner()` 取值；反序列化失败按 `FIELD_LABELS` 给中文字段级提示
  （**已知限制**：`#[serde(flatten)]` 内的字段如 `PageQuery` 只能给通用提示）。
- 返回 `ApiResponse::ok(data)`（`code=1`）/ `ApiResponse::fail(message)`（`code=0`）。
  框架层 4xx/5xx 由 `infra/catcher.rs` 统一改写为 HTTP 200 + 契约体。

### 分页
`PageQuery { page, pageSize }`（默认 1 / 10，`page_size` **clamp 到 1..=1000**）→ repo `find_page` →
`utils::page::paginate(select, db, page_index, page_size)` 是**唯一**分页执行器；出参
`PageResult { total, totalPages, items }`。分页查询必须带确定的 `ORDER BY`（约定 `order_by_desc(Id)`），
否则 LIMIT/OFFSET 会重复/漏项。

### 审计字段与人字段
- 请求体**永不接受**审计字段（`created_by` / `*_name`，防伪造）；由 repo 层盖章：create 同时写
  `created_by` + `updated_by`，update 只刷新 `updated_by`。
- 人字段拼装唯一管道：`utils::user_ref::fill_user_names(db, items, Resp::from)`，单次批量查名；
  实体/DB 用「动词过去式 + `_by`」，响应体用 `*_name`；查名**不过滤软删用户**（历史引用保留姓名）。

### 软删除与加锁读
- 主表（15 张含 `deleted_at`）查询一律 `.filter(Column::DeletedAt.is_null())`；关系表
  （`sys_role_api` / `sys_role_menu` / `sys_user_role` / `sys_user_dept` / `sys_user_position`）**硬删除**。
  `*_include_deleted` 变体只给历史 / 回收站场景。
- 「读 → 判断 → 写」不变式用 `find_by_*_for_update`（`.lock_exclusive()`）；展示查询禁止加锁。

### 常量与判定面
- 魔法字符串集中在 `src/modules/system/permission/mod.rs`：`SUPER_ROLE_KEY = "super"`、`ADMIN_USERNAME = "admin"`
  （唯一定义点；seed 侧镜像为 `SEED_SUPER_ROLE_KEY` 以避免依赖环）。
- `status` 允许值唯一来源：`dictionary::service::enabled_int_values(&db, "status") -> Vec<i8>`（读 `sys_dictionary`）。
- 授权**只有一条通道**：`ApiPermission` → `permission::service::has_api_permission`。按钮权限码（`sys_menu.permission`）
  只控前端显隐，后端不再判定（2026-09-17 起），不要重建服务层按钮码校验。

### 扩展一个新域（三步 + 两处登记）
1. `src/entity/mod.rs` 加 `pub mod <表>;`（可用 `codegen` 生成实体与骨架）
2. 容器 `mod.rs` 加 `pub mod <域>;`（`src/modules/biz/<模块>/mod.rs` 或 `src/modules/system/mod.rs`）
3. `src/modules/mod.rs` 的 `DOMAINS` 追加一行（`MountGuard::Protected` 即自动获得三件套中间件）
4. **新端点必须登记到 `src/infra/seed.rs` 的 `API_SEEDS`**，否则接口授权对它 fail-open（未登记即放行）
5. 新表迁移在 `migrations/` **追加**（不改已发布的 baseline，平台表相对顺序不动）

### 提交规范
Conventional Commits + **中文描述**，类型限 `feat` / `fix` / `refactor` / `chore` / `docs` / `test`，例如
`feat(rbac): 完善软删除过滤与用户创建校验`。PR 写明改动目的、`cargo test` 证据、契约变更（端点/响应体）及前端影响。
无 commitlint / husky，靠 review + CI。

## Important Files

| 文件 | 作用 |
|---|---|
| `src/main.rs` / `src/lib.rs` | 进程入口（tracing + `Config::load` + `app::run`）/ 库入口（6 个 pub mod） |
| `src/infra/app.rs` | 启动管线：DB → 播种 → 调度器 → AppState → 路由 → 监听 |
| `src/infra/config.rs` | `Config` + `Config::load()` 的 fail-fast 校验；真库测试 DSN 来源 |
| `src/infra/router.rs` | `build` / `build_with` / `mount_domains`：中间件与域挂载装配点 |
| `src/infra/seed.rs` | 幂等种子：`MENU_SEEDS` / `API_SEEDS`（含 `canonical_api_path` 归一）/ 字典 / 定时任务 / admin 密码 |
| `src/infra/catcher.rs` | 框架错误 → 统一契约体（HTTP 200） |
| `src/infra/state.rs` | `AppState { config, db, cache, scheduler }` + `from_depot` |
| `src/modules/mod.rs` | `DOMAINS` 登记表 + `all_domains` |
| `src/utils/{error,response,request,page,user_ref}.rs` | `AppError` / `ApiResponse` / `JsonBody` / 分页 / 人字段拼装 |
| `src/middleware/{auth,api_permission,op_log,request_timeout,cors}.rs` | 认证、接口授权、操作日志、超时、CORS |
| `src/modules/system/job/scheduler.rs` | cron 调度：300s 超时 + `catch_unwind` + 写 `sys_job_log`，cron 用本地时区 |
| `src/task/mod.rs` | 任务注册表 `handlers()` / `handler_defs()`；新增任务 = 新文件 + 一行登记 |
| `config.toml` / `.env.example` | 运行配置默认值 / compose 变量（`TIDE_JWT_SECRET` 必设） |
| `rust-toolchain.toml` / `clippy.toml` / `Cargo.toml` | 工具链钉死 / 测试放行 unwrap / lint 基线 |
| `docker/entrypoint.sh` | 容器入口：`mkdir -p uploads` → `./migration up`（`RUN_MIGRATIONS=0` 可跳过）→ `exec ./tide-server` |
| `.github/workflows/ci.yml` | lint + 真库 test 两个 job |

## Runtime/Tooling Preferences

- **Rust 刻意钉死 1.96.0**（`rust-toolchain.toml`：`profile = "minimal"`，仅 `rustfmt` + `clippy`）：
  1.96.0 与 1.96.1 的 rustfmt 结果已实际分叉，勿随手升级；无 `rustfmt.toml`，全用默认格式。
- **没有 rust-analyzer**（minimal profile）：符号/引用检索用 `graphify query/path/explain`、`graphify update .`
  或 grep / ast-grep；`graphify-out/` 是可再生索引（gitignore），其 `GRAPH_REPORT.md` 里的 "Built from commit" 需与
  `git rev-parse HEAD` 对比判断新鲜度。
- 包管理只有 cargo，无 npm/pnpm/Node 需求（前端在 `../tide-admin`）；无 Makefile / justfile / Taskfile / 活跃 git hooks。
- **Lint 基线**（`Cargo.toml`，仅根 crate；`migrations/`、`codegen/` 无 `[lints]`）：
  `unsafe_code` / `unwrap_used` / `expect_used` / `dbg_macro` / `todo` 全 `deny`；
  `clippy.toml` 只对测试（`tests` 与 `#[cfg(test)]`）放行 unwrap/expect。CI 另有全局 `RUSTFLAGS="-D warnings"`。
- **MySQL 8 + 时区**：DSN 必带 `charset=utf8mb4` 与 `timezone=%2B08:00`（缺则中文乱码 / 会话时区错）；
  本地 3307、容器内 3306、CI 127.0.0.1:3306（用 `127.0.0.1` 而非 `localhost` 避免 IPv6 歧义）；容器与 CI 另设 `TZ=Asia/Shanghai`。
- **配置覆盖约定**：`TIDE_` 前缀 + `__` 层级分隔 + 逗号分隔列表，环境变量优先于 `config.toml`。
  常用：`TIDE_ENV`、`TIDE_DATABASE__URL`、`TIDE_JWT__SECRET`、`TIDE_JWT__TTL_SECONDS`、
  `TIDE_JWT__REFRESH_TTL_SECONDS`、`TIDE_CORS__ALLOW_ORIGINS`、`TIDE_UPLOAD__DIR`、`TIDE_SEED__ENABLED`、
  `TIDE_LOG_RETENTION__{OPERATION_LOG,LOGIN_LOG,JOB_LOG,REFRESH_TOKEN}_DAYS`（`0` = 永久保留）。
- **fail-fast 校验**（`Config::load`）：`jwt.ttl_seconds > 0`、`jwt.refresh_ttl_seconds > 0`、
  `upload.max_size_mb > 0`、`upload.dir` 非空、`upload.allows` 必填；**production 语义是白名单** ——
  `env` 不属于 `{development, test}`（含未设置 / `prod` / 带空格）且密钥仍是 `dev-secret-change-me` 时**拒绝启动**。
- 播种语义：`development` 每次启动重置 `admin` 密码为 `admin123`；生产仅 `TIDE_SEED__ENABLED=true` 时播种
  （首次 bootstrap 用完即改回 `false`）。compose 里该变量写作 `TIDE_SEED_ENABLED`（单下划线），应用读的是 `TIDE_SEED__ENABLED`。
- compose：`mysql`（3307:3306）+ `backend`（`TIDE_ENV=production`，不发布端口）+ `frontend`（context `${FRONTEND_DIR:-../tide-admin}`，80:80）；
  后端健康检查 `curl -fsS -X POST .../api/v1/health | grep -q '"code":1'`。
- CI 只做 fmt / clippy / test；**不构建 `codegen/`、不构建 Docker 镜像**。

## Testing & QA

- **框架**：stock libtest + tokio，无 `sqlx::test` / `sea_orm` 测试宏 / `serial_test`，无 dev-dependencies。
- **位置**：`#[cfg(test)] mod tests` **内联在业务文件里**（`repo.rs` / `service.rs` / `validate.rs` / `api.rs` /
  `middleware/*.rs` / `task/*.rs` / `utils/*.rs`）；**没有 `tests/` 目录、没有 fixtures 目录、全仓无 mock**。
  `dto.rs` 从不写测试。当前规模（复核命令见下）：`src/` 68 个测试文件、约 520 条测试；
  `codegen/` 另有纯 `#[test]`，根 `cargo test` 不会跑它（需 `cargo test --manifest-path codegen/Cargo.toml`）。
- **属性选择**：纯逻辑用 `#[test]`；碰 DB / handler 用 `#[tokio::test]`（默认 `current_thread`）；
  真并发必须 `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`（忘了写就静默无法交错）。
- **真库夹具**（每个测试模块各自声明，不是共享工具；照抄这两段）：
  ```rust
  /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
  async fn test_db() -> sea_orm::DatabaseConnection {
      let config = crate::infra::config::Config::load().unwrap();
      Database::connect(&config.database.url).await.unwrap()
  }

  /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
  async fn test_txn() -> sea_orm::DatabaseTransaction {
      use sea_orm::TransactionTrait;
      test_db().await.begin().await.unwrap()
  }
  ```
  - 默认用 `test_txn()`：受测函数的 `*_in_tx(txn, ...)` 在测试的外层事务里跑，靠 Drop 回滚隔离。
  - **只有必须提交的数据**才用 `test_db()` + 手工清理：并发/死锁、跨连接可见性、级联、seed 幂等、middleware、
    保留期清理（`src/task/*_cleanup.rs` 用 `probe_*` 行 + 显式清理）。这类测试**共享全局行**，是「不能并行跑两个 cargo test」的根因。
  - 唯一名防撞：`format!("{prefix}_{}_{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))`（`static SEQ: AtomicU64`）。
  - 断言带中文失败信息（如 `"重复播种不应产生重复 API 记录"`）；测试名是行为句子
    （`create_role_rejects_duplicate_role_key_including_soft_deleted`）；**函数名不要与受测函数同名**（会遮蔽 `super` 导入）。
  - 基线依赖：`ACTOR_ID = 1`（种子 admin）、`sys_site_config` id=1、`super` 角色必须存在。
  - 环境变量测试必须用 `lock_env()` + `EnvGuard` 快照恢复（`src/infra/config.rs` 的 `#[allow(unsafe_code)]` 测试模块）：
    旧写法会把 CI 注入的 `TIDE_DATABASE__URL` 清掉，导致后续所有连库测试失败（2026-09-13 事故）。
- **覆盖期望**：没有覆盖率阈值、没有条数门禁。QA 门禁 = CI 的 lint job（fmt + `clippy --all-targets -- -D warnings`，
  两个 crate）+ test job（起 MySQL 8 → `cargo test --no-run` → 探活 → `migrate up` → 哨兵
  `cargo test ensure_seed_api_is_idempotent -- --nocapture --test-threads=1` → 全量 `cargo test`）。
  本地提交前跑「校验三连」，PR 附 `cargo test` 实际结论。
- 断言只针对可观察行为（行为、边界、不变式、状态迁移、真实错误），不要给实现/接线/字段搬运写测试。
- 复核规模：
  ```bash
  grep -rE '#\[tokio::test' src --include='*.rs' | wc -l
  grep -r '#\[test\]' src --include='*.rs' | wc -l
  grep -rl '#\[cfg(test)\]' src --include='*.rs' | wc -l
  ```

## Pitfalls（高危清单）

1. **fail-open 授权**：`sys_api` 里没登记的 `path + method` 对所有已登录用户放行 —— 新端点必须登记种子；
   也不要再造一个未登记的重复路由（历史 `get-depts-by-user-id` 就是真实越权口子）。
2. **路径归一**：`ApiPermission` 与 seed 两侧都必须走 `canonical_path` / `canonical_api_path`，
   否则 `/create/`、`//create`、`/%63reate` 能绕过判定。
3. **事务**：repo 层禁止新增 `db.begin()`；嵌套事务在 MySQL 下会隐式提交。service 入口的 `db` 参数类型是契约
   （自持事务用 `&DatabaseConnection`，只读用 `&impl ConnectionTrait`）。
4. **展示查询加锁**：列表/详情/名称拼装不要顺手加 `FOR UPDATE`。
5. **审计字段入参**：请求体收 `created_by` / `*_name` 即为伪造漏洞。
6. **软删除**：主表漏过滤 `deleted_at IS NULL` 会带出已删数据；关系表要硬删除。
7. **文档里的硬数字**会腐烂：改代码时同步改，或改写为不依赖数字的措辞。
8. **`AGENTS.md` 自身**：被 `.git/info/exclude` 排除（不进提交）；`docs/` 与 `graphify-out/` 是本地 / 可再生内容，
   不要当事实来源引用。
9. **`config.local.toml` 是死配置**：`Config::load` 只读 `config.toml` + `TIDE_*` 环境变量，写它没有任何效果。
10. **Dockerfile 里 `TZ` 与 `/etc/localtime` 被硬编码**、compose 的 `mysql` 与 `backend` 都假定 `Asia/Shanghai`；
    改动时区要三处一起改。
