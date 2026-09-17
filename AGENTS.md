# PROJECT KNOWLEDGE BASE

**Generated:** 2026-09-16 11:59 +0800
**Commit:** eb90c3d
**Branch:** main

## OVERVIEW

Rust + Salvo + SeaORM 的 RBAC 后端，前端 vue-vben-admin；业务按垂直切片组织、契约驱动：
接口一律 `POST + JSON body`，响应体 `{ code, data, message }`（`code=1` 成功 / `0` 失败），
HTTP 恒 200，唯一例外认证失败 401。

## STRUCTURE

```
tide-server/
├── src/                  # binary-only 根 crate（无 lib.rs）
│   ├── modules/system/   # 18 个平台域切片，四件套 api/service/repo/dto
│   ├── modules/biz/      # 业务域容器，当前仅 mod.rs（业务从这里生长）
│   ├── infra/            # 启动管线、Config、AppState、路由组装、seed
│   ├── middleware/       # InjectState / AuthRequired / op_log / CORS / 30s 超时
│   ├── entity/           # SeaORM 实体（21 表），全局共享
│   ├── task/             # 定时任务注册表 + 过期日志清理
│   └── utils/            # AppError、响应体、JWT、密码、缓存、分页、人字段拼装
├── codegen/              # 独立 crate：def JSON → entity + 四件套骨架
├── migrations/           # 独立 crate：sea-orm-migration，20 表 DDL 事实来源
├── skills/               # vendored Salvo 0.94 语料（外部参考，勿照抄）
├── docker/entrypoint.sh  # 容器内迁移 + 启动
├── uploads/              # gitignored，运行时上传产物
└── graphify-out/         # gitignored，生成产物
```

三个独立包、非 workspace：根 crate / `migrations/` / `codegen/`。

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| 服务入口 `src/main.rs`、mod 声明、依赖方向 | `src/AGENTS.md` | binary-only，6 私有 mod，`task/` 注册表规则 |
| 新增业务域、路由装配 | `src/modules/AGENTS.md` | `DOMAINS` 登记表 + `MountGuard`，三步装配 |
| 平台域切片：分层 / repo 边界 / 加锁读 / 软删除 / 命名 / 测试 | `src/modules/system/AGENTS.md` | 18 域硬规则表 |
| 启动顺序、配置、全局状态、种子 | `src/infra/AGENTS.md` | `Config` fail-fast、`AppState`、catcher |
| 错误码、响应体、分页、人字段拼装 | `src/utils/AGENTS.md` | 拼装协议唯一实现 |
| 认证 / 接口授权 / 操作日志 / 超时豁免 | `src/middleware/AGENTS.md` | 上传下载两个流式端点按路径豁免 30s |
| 表结构与实体来源 | `src/entity/AGENTS.md` | 21 表、三种生成来源、无物理外键 |
| 生成新域骨架 | `codegen/AGENTS.md` | def JSON → entity + 四件套 |
| 建表与迁移 | `migrations/AGENTS.md` | 占位在前 + baseline 建 20 表 |
| Salvo API 用法查询 | `skills/AGENTS.md` | 0.94 语料 vs 项目 0.95 |

## CODE MAP

refs = 实测 `grep -ro` 次数 / 文件数；Location 相对 `src/`。

| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `AppError` | enum | `utils/error.rs:21` | 491 / 37 | 唯一业务错误；收 1213/1205 |
| `AppState` | struct | `infra/state.rs:12` | 166 / 30 | config+db+cache+scheduler |
| `ApiResponse<T>` | struct | `utils/response.rs:11` | 138 / 25 | `{code,data,message}` 包裹 |
| `JsonBody<T>` | extractor | `utils/request.rs:66` | 116 / 20 | 手写提取器 + 字段级报错 |
| `ApiResult<T>` | alias | `utils/mod.rs:24` | 111 / 17 | handler 返回类型 |
| `Config::load` | fn | `infra/config.rs` | 49 / 37 | 配置装载；真库测试 DSN 源 |
| `fill_user_names` | fn | `utils/user_ref.rs:58` | 66 / 15 | 人字段显示名唯一管道 |
| `PageQuery`/`PageResult`/`paginate` | struct+fn | `utils/page.rs:13/39/91` | 56 / 46 / 15 | 分页三件，`clamp(1,1000)` |
| `SUPER_ROLE_KEY` | const | `modules/system/permission/mod.rs` | 44 / 9 | 超管保留字：短路 + 改名保护 |
| `enabled_int_values` | fn | `modules/system/dictionary/service.rs:214` | 31 / 18 | `status` 允许值唯一来源 |
| `DOMAINS` | const 表 | `modules/mod.rs:78` | 9 / 5（仅 router 消费） | 19 行路由装配入口 |

## CONVENTIONS

只列偏离 Rust / Salvo / SeaORM 默认的项目级约定，细则在指到的子档。

- 工具链钉死：Rust 1.96.0（与 1.96.1 的 rustfmt 结果已分叉）、edition 2024、Salvo 0.95 仅用
  `oapi`、SeaORM 1.x（`sqlx-mysql` + `runtime-tokio`）、MySQL 8 → `src/AGENTS.md`
- 分层方向：`api → service → repo → entity`；`utils` 与 `infra`(config、state) 被各层依赖；例外
  ——`middleware`、`task/*`、`infra::seed`、`job/scheduler.rs` 可直调目标域 repo
  → `src/modules/system/AGENTS.md`
- lint：`Cargo.toml` 对 `unsafe_code` / `unwrap_used` / `expect_used` / `dbg_macro` / `todo` 全
  `deny`；`clippy.toml` 只放行测试代码
- 注释用简体中文、中英文之间留空格；标识符用英文命名
- handler 返回 `ApiResult<T>`，业务错误用 `AppError`，repo 一律 `?` 传播 → `src/utils/AGENTS.md`
- 超管角色键等魔法字符串集中为常量（`SUPER_ROLE_KEY`、`ADMIN_USERNAME`）；按钮权限码不再有后端常量（判定面只消费接口通道，2026-09-17 起）
- 契约例外：`file/upload` 走 multipart，`file/download` 与 `site-config/get` 是 GET；OpenAPI 在
  `/api-doc/openapi.json` + `/swagger-ui`，只收 `#[endpoint]`；vben v5 默认 `successCode=0`，前端
  `request.ts` 必须显式改为 `code === 1`
- 域内命名与顺序（`find_page` / `page_<实体>` / `list_<实体>`；api.rs 顺序 = 路由挂载顺序）、
  软删除（主表过滤 `deleted_at IS NULL`、关系表硬删除）、加锁读（2026-09-14：「读 → 判断 →
  写」不变式一律 `SELECT ... FOR UPDATE`）、测试规范（内联各域 `repo.rs`/`service.rs`、直连
  MySQL、`test_txn()` 回滚隔离）→ `src/modules/system/AGENTS.md`
- 人字段：`动词过去式_by` + Resp `*_name`，后端批量拼装 → `src/utils/AGENTS.md`

## ANTI-PATTERNS (THIS PROJECT)

- **避免不必要的全量打包编译（2026-09-10）**：不主动跑 `cargo build` / `cargo build --release` /
  `cargo build --workspace`，仅部署 / 产物验证（Dockerfile、CI）时才执行
- repo 里禁止业务保留字判断、业务错误文案、唯一性 / 状态机决策、跨域调用他域 repo/service
  → `src/modules/system/AGENTS.md`
- 业务层新增函数禁止新增 `db.begin()`：需要多表原子性先把事务边界上移 service
- 展示类查询（列表 / 详情 / 名称拼装）一律普通读，不要顺手加锁
- 请求体不接受人字段（防伪造），审计字段由 repo 层统一盖章
- `skills/` 是 Salvo 0.94 外部语料，禁止照抄其 API 到本项目的 0.95
- README 的数字已过时（`#[test]` 实为 489、`API_SEEDS` 实为 87），勿以 README 为准

## UNIQUE STYLES

- 垂直切片 + `DOMAINS` 登记表装配，新增域三步走 → `src/modules/AGENTS.md`
- 单信封契约：HTTP 恒 200，业务成败只看 `code`
- Conventional Commits + 中文描述，如 `feat(rbac): 完善软删除过滤与用户创建校验`；类型
  `feat` / `fix` / `refactor` / `chore` / `docs` / `test`
- PR 说明改动目的、附验证证据（`cargo test` 结果）、契约变更同步说明响应体与端点
- 固定分工：AI 编写失败测试并做最终 review，用户手动实现业务代码
- 新功能先建设计文档（`docs/superpowers/specs/`），按测试红 → 实现 → 验证推进
- 全仓 489 个 `#[test]`，全连真库、无 mock

## COMMANDS

```bash
cargo fmt --check   # 格式校验（rustfmt）
cargo check         # 只做编译检查，不产出二进制
cargo test          # 全量测试，需本地 MySQL：docker compose up -d
cargo test role     # 运行单个模块
cd migrations && DATABASE_URL='mysql://root:root@localhost:3307/tide_server' cargo run -- up
cargo run           # 启动服务，监听 0.0.0.0:8080
# 不跑 cargo build / cargo build --release / cargo build --workspace（仅部署 / 产物验证）
```

## NOTES

- 无 rust-analyzer（`profile=minimal`）：符号与引用检索用 `graphify query/path/explain` 或 ast-grep + grep；
  `graphify-out/` 是可再生的本地索引（gitignore、零 API 费用），`graphify update .` 做 AST 增量刷新
- sea-orm 1.1.20 真库无 savepoint：事务内嵌套 `begin()` 会被 MySQL 隐式提交 → 自持事务的
  service 入口收 `&DatabaseConnection`（清单见 `src/modules/system/AGENTS.md`）；旧 root 记的
  repo 层 `*_with_links` / `soft_delete_*` 例外已过期
- DSN 必带 `charset=utf8mb4` 与 `timezone=%2B08:00`；容器 / CI 另设 `TZ=Asia/Shanghai`
- 开发环境每次启动重置 `admin` 密码为 `admin123`；生产靠 `TIDE_SEED__ENABLED` 关种子
- 根目录的 `cargo run -- up` 会启动服务器而非迁移；迁移命令必须在 `migrations/` 目录执行
- CI：`RUSTFLAGS="-D warnings"`，fmt / clippy 对根 crate 与 `migrations/` 各跑一次；test job 把
  `config.toml` 的 3307 换成 `127.0.0.1:3306`、起 MySQL 8、migrate up、哨兵
  `ensure_seed_api_is_idempotent --test-threads=1` 后跑全量；CI 与 Dockerfile 都不构建 `codegen/`
- 本地 compose 宿主 3307 → 容器 3306；首次 bootstrap 加 `TIDE_SEED_ENABLED=true`
- 测试与迁移需要连接本地 MySQL，在非沙箱环境执行
- 1213 / 1205 由 `utils/error.rs` 映射成「操作冲突，请稍后重试」→ `src/utils/AGENTS.md`
- 已下移到子档、root 只留指针：分层依赖与 repo「只拼 SQL」边界（含 2026-09-10 三类反例）、
  并发一致性加锁读（2026-09-14）、软删除、切片内命名与顺序、测试规范 →
  `src/modules/system/AGENTS.md`；人字段命名与名称拼装协议、1213/1205 映射 →
  `src/utils/AGENTS.md`；目录细节与 skills 语料风险 → 各目录档
