# PROJECT KNOWLEDGE BASE

**Updated:** 2026-09-19（置顶新增最高规则：AI 只写测试与骨架、实现由作者手写；二轮复查：CODE MAP 引用计数全表重测，统一为 grep -ro/-rl 可复现口径）

## 最高规则（优先于本文件其余全部内容）

**本项目是辅助学习 Rust 的练手项目。固定分工，AI 不得越界：**

- **AI 负责**：所有测试（失败测试先行 + 断言用例 + 真库夹具）、代码骨架（文件/模块结构、
  类型与函数签名、trait/impl 接线、`?` 错误传递、doc 注释与实现提示）、最终 code review。
- **人负责**：具体业务实现（函数体逻辑）。AI 交付到「可编译的骨架 + 失败的测试」为止，
  不得顺手写完实现。
- **例外（不算业务实现，AI 可直接完成）**：配置文件、迁移 DDL、种子数据、机械登记行
  （`mod` 声明、`DOMAINS` 追加、`API_SEEDS` / `MENU_SEEDS` 条目）、文档。
- **骨架体不得用 `todo!()`**：`Cargo.toml` 对 `todo` 已 `deny`。骨架函数体写成可编译的桩
  （返回 `Default` / 空集合 / `AppError::Biz("未实现")`），并用 `// 实现提示：…` 注释标出该写什么；
  测试保持红灯即为交接信号。
- 若某任务在不写实现的前提下无法交付可编译状态，先向用户确认，再动实现代码。

## OVERVIEW

Rust + Salvo + SeaORM 的 RBAC 后端，前端 vue-vben-admin；业务按垂直切片组织、契约驱动：
接口一律 `POST + JSON body`，响应体 `{ code, data, message }`（`code=1` 成功 / `0` 失败），
HTTP 恒 200，例外仅认证失败 401 与 CORS 预检 204/403。

## STRUCTURE

```
tide-server/
├── src/                  # lib + bin 双 target（bin 通过 lib.rs 取模块）
│   ├── lib.rs            # 库入口：6 个 pub mod（entity/infra/middleware/modules/task/utils），供 bin target 取用
│   ├── main.rs           # 进程入口：tracing + Config::load → infra::app::run
│   ├── modules/system/   # 18 个平台域切片，四件套 api/service/repo/dto
│   ├── modules/biz/      # 业务域容器，当前仅 mod.rs（业务从这里生长）
│   ├── infra/            # 启动管线、Config、AppState、路由组装、seed
│   ├── middleware/       # InjectState / AuthRequired / op_log / CORS / 30s 超时
│   ├── entity/           # SeaORM 实体（21 表），全局共享
│   ├── task/             # 定时任务注册表 + 4 个过期日志清理
│   └── utils/            # AppError、响应体、JWT、密码、缓存、分页、人字段拼装
├── codegen/              # 独立 crate：def JSON → entity + 四件套骨架
├── migrations/           # 独立 crate：sea-orm-migration，21 表 DDL 事实来源
├── docker/entrypoint.sh  # 容器内迁移 + 启动
└── uploads/、graphify-out/  # 均 gitignored：运行时上传产物 / 可再生索引
```

三个独立包、非 workspace：根 crate / `migrations/` / `codegen/`。

`src/modules/biz/` 是业务域容器（当前仅 `mod.rs`）：具体业务功能在这里生长，
平台能力（RBAC / 认证 / 字典 / 日志 / 文件 / 组织）在 `src/modules/system/` 下。

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| 服务入口 `src/main.rs`、mod 声明、依赖方向 | `src/AGENTS.md` | lib + bin 双 target，`task/` 注册表规则 |
| 新增业务域、路由装配 | `src/modules/AGENTS.md` | `DOMAINS` 登记表 + `MountGuard` 三步装配，路由自动挂载 |
| 平台域切片：分层 / repo 边界 / 加锁读 / 软删除 / 命名 / 测试 | `src/modules/system/AGENTS.md` | 18 域硬规则表 |
| 启动顺序、配置、全局状态、种子 | `src/infra/AGENTS.md` | `Config` fail-fast、`AppState`、catcher |
| 错误码、响应体、分页、人字段拼装 | `src/utils/AGENTS.md` | 拼装协议唯一实现 |
| 认证 / 接口授权 / 操作日志 / 超时豁免 | `src/middleware/AGENTS.md` | 上传下载两个流式端点按路径豁免 30s |
| 表结构与实体来源 | `src/entity/AGENTS.md` | 21 表、三种生成来源、无物理外键 |
| 生成新域骨架 | `codegen/AGENTS.md` | def JSON → entity + 四件套 |
| 建表与迁移 | `migrations/AGENTS.md` | baseline 20 表 + `sys_refresh_token` 后续迁移 |

## CODE MAP

无 rust-analyzer（toolchain `profile=minimal`）：refs 为 grep 实测，仅 `src/`、子串匹配（非
全词、非 LSP）——次数 `grep -ro '<sym>' src --include='*.rs' | wc -l`，文件数同式换 `-rl`。
Location 相对 `src/`。

| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `AppError` | enum | `utils/error.rs:21` | 501 / 38 | 唯一业务错误；收 1213/1205 |
| `AppState` | struct | `infra/state.rs:12` | 199 / 31 | config+db+cache+scheduler |
| `ApiResponse<T>` | struct | `utils/response.rs:11` | 140 / 25 | `{code,data,message}` 包裹 |
| `JsonBody<T>` | extractor | `utils/request.rs:66` | 116 / 20 | 手写提取器 + 字段级报错 |
| `ApiResult<T>` | alias | `utils/mod.rs:24` | 111 / 17 | handler 返回类型 |
| `Config::load` | fn | `infra/config.rs:237` | 61 / 45 | 配置装载；真库测试 DSN 源 |
| `fill_user_names` | fn | `utils/user_ref.rs:58` | 66 / 15 | 人字段显示名唯一管道 |
| `PageQuery` | struct | `utils/page.rs:13` | 56 / 22 | 分页入参；`page_size` `clamp(1,1000)` |
| `PageResult<T>` | struct | `utils/page.rs:39` | 46 / 15 | 分页出参 |
| `paginate` | fn | `utils/page.rs:91` | 18 / 15 | 分页执行；各域 repo 统一调用 |
| `SUPER_ROLE_KEY` | const | `modules/system/permission/mod.rs:19` | 41 / 9 | 超管保留字：短路 + 改名保护 |
| `enabled_int_values` | fn | `modules/system/dictionary/service.rs:214` | 31 / 18 | `status` 允许值唯一来源 |
| `DOMAINS` | const 表 | `modules/mod.rs:84` | 24 / 7 | 19 行内置路由装配入口 |
| `all_domains` | fn | `modules/mod.rs:259` | 7 / 2 | 合并内置域与额外传入的域（无 extra 时等于内置） |

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
- 超管角色键等魔法字符串集中为常量（`SUPER_ROLE_KEY`、`ADMIN_USERNAME`，唯一定义在
  `modules/system/permission/mod.rs`）；按钮权限码不再有后端常量（判定面只消费接口通道，2026-09-17 起）
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
- 文档里的硬数字（测试条数、种子条数、引用计数、行号）会随切片增长而腐烂：改代码时同步改数字，
  或改写不依赖具体数字的措辞

## UNIQUE STYLES

- 垂直切片 + `DOMAINS` 登记表装配：新增域三步走，路由自动挂载 → `src/modules/AGENTS.md`
- 单信封契约：HTTP 恒 200，业务成败只看 `code`
- Conventional Commits + 中文描述，如 `feat(rbac): 完善软删除过滤与用户创建校验`；类型
  `feat` / `fix` / `refactor` / `chore` / `docs` / `test`
- PR 说明改动目的、附验证证据（`cargo test` 结果）、契约变更同步说明响应体与端点
- 固定分工：见文首「最高规则」——AI 写测试与骨架、作者手写实现；本条是其摘要
- 新功能先建设计文档（`docs/superpowers/specs/`，gitignored 本地目录），按测试红 → 实现 → 验证推进
- 全仓 `#[test]` 全连真库、无 mock；数量随切片增长，别把条数当事实写进文档

## COMMANDS

```bash
cargo fmt --check   # 格式校验（根 crate 与 migrations/ 各跑一次）
cargo check         # 只做编译检查，不产出二进制
cargo test          # 全量测试，需本地 MySQL：docker compose up -d（宿主 3307 → 容器 3306）
cargo test --lib    # 仅 lib target 的测试
cargo test role     # 运行单个模块；勿同时并行第二个 cargo test（共享种子行）
cd migrations && DATABASE_URL='mysql://root:root@localhost:3307/tide_server?charset=utf8mb4&timezone=%2B08:00' cargo run -- up
cargo run           # 启动服务，监听 0.0.0.0:8080
# 不跑 cargo build / --release / --workspace（仅部署 / 产物验证）；codegen 用 --manifest-path codegen/Cargo.toml
```

## NOTES

- 无 rust-analyzer（`profile=minimal`）：符号与引用检索用 `graphify query/path/explain` 或 ast-grep + grep；
  `graphify-out/` 是可再生的本地索引（gitignore、零 API 费用），`graphify update .` 做 AST 增量刷新
- sea-orm 1.1.20 真库无 savepoint：事务内嵌套 `begin()` 会被 MySQL 隐式提交 → 自持事务的
  service 入口收 `&DatabaseConnection`（清单见 `src/modules/system/AGENTS.md`）
- DSN 必带 `charset=utf8mb4` 与 `timezone=%2B08:00`；容器 / CI 另设 `TZ=Asia/Shanghai`
- 开发环境每次启动重置 `admin` 密码为 `admin123`；生产仅当 `seed.enabled`（`TIDE_SEED__ENABLED=true`）才播种
- 根目录的 `cargo run -- up` 会启动服务器而非迁移；迁移命令必须在 `migrations/` 目录执行
- CI：`RUSTFLAGS="-D warnings"`，fmt / clippy 对根 crate 与 `migrations/` 各跑一次；test job 把
  `config.toml` 的 3307 换成 `127.0.0.1:3306`、起 MySQL 8、migrate up、哨兵
  `ensure_seed_api_is_idempotent --test-threads=1` 后跑全量；CI 与 Dockerfile 都不构建 `codegen/`
- 健康检查必须读响应体：HTTP 恒 200，compose 探活为 `curl -X POST /api/v1/health | grep '"code":1'`
- 首次 bootstrap 置 `TIDE_SEED_ENABLED=true`（compose 映射为 `TIDE_SEED__ENABLED`）；production 必设 `TIDE_JWT_SECRET`（拒默认密钥）
- 已下移到子档、root 只留指针：分层依赖与 repo「只拼 SQL」边界、加锁读、软删除、切片内命名与
  顺序、测试规范 → `src/modules/system/AGENTS.md`；人字段拼装协议、1213/1205 映射 →
  `src/utils/AGENTS.md`；目录细节风险 → 各目录档
