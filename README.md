<div align="center">

# tide-server

基于 **Rust · Salvo · SeaORM** 的 RBAC 中后台管理系统后端，
配套前端 [tide-admin](https://github.com/xqh-jason/tide-admin)（其基座为 Vue Vben Admin 5.x）

[![CI](https://github.com/xqh-jason/tide-server/actions/workflows/ci.yml/badge.svg)](https://github.com/xqh-jason/tide-server/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![rust](https://img.shields.io/badge/rust-1.96.0-orange.svg)](./rust-toolchain.toml)
[![salvo](https://img.shields.io/badge/salvo-0.95-red.svg)](https://github.com/salvo-rs/salvo)
[![sea-orm](https://img.shields.io/badge/sea--orm-1.x-green.svg)](https://github.com/SeaQL/sea-orm)
[![mysql](https://img.shields.io/badge/mysql-8-4479a1.svg)](https://www.mysql.com/)

</div>

## ✨ 功能特性

**认证与会话**

- 账号密码 + 图形验证码登录，失败文案统一防账号枚举，分级落登录日志
- 双凭证机制：2h access JWT + 7 天 refresh token（HttpOnly Cookie，SHA-256 落库不存明文）
- 静默刷新：`POST /auth/refresh` 凭 Cookie 换发新 access token（响应体为裸 token，
  失败真 HTTP 401）；角色随刷新从 DB 重查，权限变更最迟一个 token 周期生效
- 服务端可吊销：登出 / 管理员强制下线 / 会话管理页，吊销即时生效、重启不丢
- 过期凭证每日定时物理清理（保留 30 天供审计，天数可配、`0` = 永久保留）

**权限（RBAC 三级同源）**

- 用户 / 角色 / 菜单 / API 四大管理，接口·菜单·按钮权限码统一在种子中登记（按钮码只控前端显隐）
- 两级鉴权：认证中间件（会话合并查询，禁用/软删用户即时 401）+
  接口级授权（路径规范化后按 `sys_api` path+method 精确匹配 + 角色绑定，未登记放行、超管短路）

**系统管理**

- 部门（树表）/ 职位 / 数据字典（类型 + 字典项）/ 参数配置 / 文件上传 / 定时任务
- 审计盖章（`created_by` / `updated_by`）与操作人名称批量拼装，请求体脱敏截断落操作日志
- 操作日志**只记写请求**（非 POST 与读语义路径如 `/list`、`/get`、`/info`、`/download` 不落库），
  消除只读翻页带来的写放大；授权失败的写请求仍留痕
- 四类日志/会话保留期独立可配（操作日志 / 登录日志 / 调度日志 / 过期会话，`0` = 永久保留），
  过期清理任务每日物理删除
- 主表软删除、关系表硬删除约定贯穿全部查询

**工程化**

- 统一契约：`POST + JSON`，响应 `{ code, data, message }`（`code=1` 成功），
  HTTP 恒 200，仅认证失败 401；Swagger UI 开箱可用
- SeaORM 迁移（独立 crate，含列表查询复合索引）+ 启动幂等种子（菜单树 46 条 /
  API 权限点 87 条 / 默认定时任务）
- 分页统一按主键降序（`ORDER BY id DESC`）：无排序时 LIMIT/OFFSET 行序由执行计划决定，
  会出现跨页重复与漏项
- 真库集成测试内联在各域（事务回滚隔离，无孤儿数据）；clippy 对 unwrap / expect / todo /
  unsafe 全量 deny

## 🖼 界面预览

点击缩略图查看原图（支持键盘操作与全屏查看）。

| 登录页 | 角色管理 |
| :---: | :---: |
| [![登录页](screenshots/01-login.png)](screenshots/01-login.png) | [![角色管理](screenshots/02-role.png)](screenshots/02-role.png) |
| 账号密码 + 图形验证码登录 | 角色列表，内置 `super` 超管角色不可编辑 |

| 菜单管理 | 定时任务 |
| :---: | :---: |
| [![菜单管理](screenshots/03-menu.png)](screenshots/03-menu.png) | [![定时任务](screenshots/04-job.png)](screenshots/04-job.png) |
| 菜单树与按钮权限码登记（只控前端显隐） | Cron 调度，支持立即执行与执行日志 |

| API 管理（接口权限点登记，种子 87 条，鉴权判定的数据源） | |
| :---: | :---: |
| [![API 管理](screenshots/05-api.png)](screenshots/05-api.png) | |

## 🧱 技术栈

| 层 | 选型 |
|---|---|
| Web 框架 | Salvo 0.95（oapi OpenAPI 契约） |
| ORM / 迁移 | SeaORM 1.x / sea-orm-migration（独立 crate `migration`，目录 `migrations/`） |
| 认证 | 双凭证 JWT（jsonwebtoken）+ 自研 RBAC |
| 定时任务 | tokio-cron-scheduler + `sys_job` 调度注册表 |
| 配置 | config-rs（`config.toml` + `TIDE_` 环境变量覆盖） |
| 数据库 | MySQL 8（utf8mb4） |
| 部署 | Docker 多阶段构建 + docker-compose + GitHub Actions |

## 🚀 快速开始

| 环境要求 | 版本 |
|---|---|
| Rust | 1.96.0（`rust-toolchain.toml` 已钉住，`rustup` 自动对齐） |
| MySQL | 8.x（推荐经 docker-compose 起本地实例） |
| 前端（可选） | Node `^22.18 \|\| ^24.12` + pnpm 11 |

```bash
# 1. 起开发 MySQL（3307 映射到宿主机；首次会自动建库）
docker compose up -d mysql

# 2. 建表（在 migrations 目录执行；注意根目录 cargo run 会启动服务）
cd migrations
DATABASE_URL='mysql://root:root@localhost:3307/tide_server' cargo run -- up
cd ..

# 3. 启动后端（监听 0.0.0.0:8080，development 环境自动执行幂等种子）
cargo run

# 4. 前端（tide-admin 仓库，与后端同级存放）
cd ../tide-admin
pnpm install
pnpm dev:ele    # http://localhost:5910，/api 代理 → http://127.0.0.1:8080
```

默认账号 `admin / admin123`（development 环境种子约定，每次启动重置；生产环境务必第一时间改密）。

## 🐳 Docker 一键部署

前后端仓库需同级存放：`tide-server/` 与 `tide-admin/`。

```bash
cp .env.example .env      # TIDE_JWT_SECRET 必设（生产用强随机串），密码按需修改
docker compose up -d --build
```

- 前端：http://localhost（nginx 静态托管 + `/api` 反代后端）
- 健康检查：http://localhost/api/v1/health
- 后端容器启动时自动执行迁移；上传文件落在 `backend_uploads` 卷

### 首次部署 bootstrap（创建初始 admin）

生产模式默认不执行种子（避免弱口令重置）。首次部署需显式开启一次：

```bash
TIDE_SEED_ENABLED=true docker compose up -d --build backend   # 建初始 admin / super 角色
docker compose up -d backend                                  # 改回默认后重启（不再重置密码）
```

随后**立即登录并修改 admin 密码**。

### ⚠️ 上线前必做的三件事（安全部署须知）

本项目默认值面向**本地开发**，直接上公网会失守。部署到生产前逐项确认：

1. **关闭强口令重置种子**：保持 production 环境不设 `TIDE_SEED__ENABLED`（默认即关），
   否则每次重启都会把 `admin` 密码重置为 `admin123`；bootstrap 完成后立即改密。
2. **换掉 JWT 密钥**：`TIDE_JWT__SECRET` 必须是强随机串。`production` 环境下仍用内置
   开发密钥，服务会**拒绝启动**（fail-fast，属于有意设计）。
3. **确认上传目录与保留期**：`TIDE_UPLOAD__DIR` 指向的目录不要对外静态托管；
   按合规要求设置 `TIDE_LOG_RETENTION__*_DAYS`（`0` = 永久保留）。

另：接口鉴权对**未登记的 `path + method` 一律放行**（fail-open，便于逐域接管）。
标准部署应保留种子数据中的 `API_SEEDS` 登记（87 条）；如果清空了 `sys_api` 表，
所有已登录用户将可调用全部接口。

### 环境变量（`TIDE_` 前缀 + `__` 层级分隔）

| 环境变量 | 对应配置 | 说明 |
|---|---|---|
| `TIDE_ENV` | `env` | `development` / `production`（生产不重置 admin 弱口令） |
| `TIDE_SEED__ENABLED` | `seed.enabled` | 生产环境显式开启启动种子（首次部署 bootstrap 用） |
| `TIDE_DATABASE__URL` | `database.url` | 连接串，务必带 `charset=utf8mb4&timezone=%2B08:00` |
| `TIDE_JWT__SECRET` | `jwt.secret` | JWT 签名密钥（compose 部署必设），生产替换为强随机串；production 下默认开发密钥会被拒绝启动 |
| `TIDE_JWT__TTL_SECONDS` | `jwt.ttl_seconds` | access token 过期秒数（自动识别数字类型） |
| `TIDE_JWT__REFRESH_TTL_SECONDS` | `jwt.refresh_ttl_seconds` | 刷新凭证有效期秒数 |
| `TIDE_CORS__ALLOW_ORIGINS` | `cors.allow_origins` | 跨域白名单，逗号分隔（如 `a.com,b.com`）；配置文件内置的是 vben 默认的 5173，tide-admin 开发端口是 **5910**——经 Vite proxy 同源访问时不需改，若前端直连后端（非代理）则需把源站加进来 |
| `TIDE_UPLOAD__DIR` | `upload.dir` | 上传落盘目录 |
| `TIDE_LOG_RETENTION__OPERATION_LOG_DAYS` | `log_retention.operation_log_days` | 操作日志保留天数，默认 90；`0` = 永久保留 |
| `TIDE_LOG_RETENTION__LOGIN_LOG_DAYS` | `log_retention.login_log_days` | 登录日志保留天数，默认 90；`0` = 永久保留 |
| `TIDE_LOG_RETENTION__JOB_LOG_DAYS` | `log_retention.job_log_days` | 调度日志保留天数，默认 90；`0` = 永久保留 |
| `TIDE_LOG_RETENTION__REFRESH_TOKEN_DAYS` | `log_retention.refresh_token_days` | 已过期会话保留天数，默认 30；`0` = 永久保留 |

列表类字段统一逗号分隔；同源反代部署下无需配置 CORS。

四类保留期分开配置而非共用一个天数：过期会话过期即不可用（保留仅供审计「谁被何时下线」），
而审计日志是合规证据，通常需要更长窗口。

## 📡 接口契约

- 所有端点 `POST + JSON body`（文件上传为 multipart），响应体统一
  `{ code: 1, data, message }`（`code=1` 成功 / `0` 失败），HTTP 恒 200，仅认证失败 401
- 分页请求 `{ page, pageSize }`，响应 `{ total, totalPages, items }`；结果统一按 `id` 降序
- 操作日志的 `keyword` 为路径**前缀**匹配（如 `/api/v1/user`），前缀匹配才能利用 B-Tree 索引
- Swagger UI：`/swagger-ui`（规范文件 `/api-doc/openapi.json`）

## 📁 目录结构

```
src/
├── lib.rs                                    # 基座公开面（外部项目可依赖的入口，2026-09-18 起）
├── main.rs                                   # 进程入口：tracing + Config::load → infra::app::run
├── modules/system/<域>/                      # 平台能力域（垂直切片契约驱动四件套，18 个）
├── modules/biz/<域>/                         # 业务域容器（具体业务功能从这里生长）
├── infra/                                    # 启动管线、配置、AppState、路由登记表
├── middleware/                               # AuthRequired / OperationLog / ApiPermission
├── entity/                                   # SeaORM 实体（全局共享）
├── utils/                                    # 错误、响应体、JWT、密码、缓存、分页、人名字段拼装
└── task/                                     # 四类保留期清理任务（job 域注册，天数读配置）
migrations/          # sea-orm-migration（独立 crate，包名 `migration`，含列表查询复合索引）
codegen/             # 代码生成器（entity / 四件套骨架）
docker/              # 容器入口脚本
```

## 🧩 作为基座：在本仓开发，或派生你自己的系统

本仓是**平台能力基座**：认证 / RBAC / 字典 / 日志 / 定时任务 / 文件 / 组织都已就绪，
且与具体业务无关。**两种用法都支持，按你的想法选**：

| 用法 | 适合 | 怎么做 |
|---|---|---|
| **A. 直接 clone 本仓开发** | 你只有一个业务系统；想要最少的仓库与配置 | clone 后把业务域写进 `src/modules/biz/<域>/`，在 `DOMAINS` 加一行 |
| **B. 作为依赖（独立仓库）** | 你要同时养多个业务系统；需要各自的发版节奏 | 自己的仓库用 **git 依赖 + 版本 tag** 引用本仓，用 `run_with_domains` 注册自己的域 |

两者都不需要 fork 基座去改核心机制。**若你选 B，本仓保持业务中性**：业务表（前缀随你自定）
属于你自己的 crate 与迁移，不回流本仓——这样一个基座可以同时服务多个业务系统。
（选 A 则是你把这仓当成自己项目的起点，`biz/` 与 `DOMAINS` 都可以按需改。）

### 用法 A：直接在本仓开发

```
src/modules/biz/<你的域>/    # 四件套 api/service/repo/dto，写法与 system/ 下的域完全一致
src/entity/<你的表>.rs        # 实体（可用 codegen 生成）
migrations/                   # 业务表迁移（追加新文件，不改 baseline）
```

接入三处：`src/entity/mod.rs` 加 `pub mod <表>;` → `src/modules/biz/mod.rs` 加
`pub mod <域>;` → `src/modules/mod.rs` 的 `DOMAINS` 加一行。前端页面放
`tide-admin` 的 `apps/web-ele/src/views/biz/<域>/`。

### 用法 B：派生独立业务仓

#### 后端仓

```toml
# <你的仓库>/Cargo.toml
[dependencies]
tide-server = { git = "https://github.com/<owner>/tide-server", tag = "v0.2.0" }
salvo = { version = "0.95", features = ["oapi"] }
sea-orm = { version = "1", features = ["sqlx-mysql", "runtime-tokio", "macros", "with-chrono", "with-json"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

```rust
// <你的仓库>/src/main.rs —— 用自己的业务域启动基座
use salvo::prelude::Router;
use tide_server::infra;
use tide_server::modules::{DomainMount, MountGuard};

mod my_domain; // 你自己的业务域（四件套，写法与基座内置域一致）

/// 你自己的域：`Protected` 自动获得鉴权 / 操作日志 / 接口授权三件套。
const MY_DOMAINS: &[DomainMount] = &[DomainMount {
    path: "<域前缀>",
    guard: MountGuard::Protected,
    routers: &[my_domain::routes],
}];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let config = infra::config::Config::load()?;
    infra::app::run_with_domains(config, MY_DOMAINS).await
}
```

要点：

- **不必接鉴权**：业务域走完 `Protected` 后与平台域同权，中间件、超时豁免、统一错误体都自动生效；
- **别忘了登记接口权限点**：新端点要进 `sys_api`（未登记接口按 fail-open 放行）；
- 配置仍从**进程工作目录**的 `config.toml` 读（`TIDE_*` 环境变量可覆盖全部项）；
- 基座升级 = 改 tag；基座当前**不承诺跨 tag 的 API 兼容**，升版时预期需小改。

#### 前端仓

从 [tide-admin](https://github.com/xqh-jason/tide-admin) fork 一份（它的 `apps/web-ele`
是基座管理界面），然后把新页面放进 `apps/web-ele/src/views/<分组>/<域>/`：

- 页面三件：`index.vue`（页面 + vxe-table）+ `data.ts`(列 / 搜索 / 表单 schema)
  + `modules/form.vue`（新建 / 编辑抽屉）；
- `apps/web-ele/src/api/<域>.ts` 一个资源一个文件；
- **菜单由后端驱动**（`accessMode: backend`）：在 `MENU_SEEDS` 里加页面行，
  `component` 写 `#/views/<分组>/<域>/index.vue`（会被 `normalizeComponent`
  归一并命中 `import.meta.glob`）；
- 页面也可以直接放在基座的 `tide-admin` 里（用法 A），此时菜单种子同样在基座。

### 基座发版（tag 流程）

只要有人用 tag 引用本仓，tag 就是发布点（用法 A 不需要发版）。**先过门禁、再打 tag**：

```bash
# 1. 三道门禁全绿（CI 对 tag 也会跑，但本地先跑一轮能省一个来回）
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                      # 需本地 MySQL

# 2. 打 tag（推送到远端后消费方才能解析）
git tag vX.Y.Z
git push origin main --tags
```

消费方升级 = 改 `Cargo.toml` 的 `tag` 后 `cargo update -p tide-server`。

**为什么强调“先过门禁”**：2026-09-18 实际踩过——tag 已打好、消费方也拉到了，
才发现该提交在 `-D warnings` 下不干净（一个 clippy 告警），只能 `git tag -d` 重打。
现在 CI 监听 `tags: ["v*"]`，tag 自身也有校验，但 CI 是事后发现，本地门禁是事前。

### 部署

`docker-compose.yml` 的 `frontend.build.context` 可配置（用法 A 用默认值即可）：

```bash
FRONTEND_DIR=<你的前端目录> docker compose up -d --build
```

默认值 `../tide-admin` 即本仓自带前端。

## 🧪 测试与 CI

集成测试内联在各域 `repo.rs` / `service.rs`，直连本地 MySQL；测试夹具用事务回滚
隔离（`test_txn()`），结束（含 panic）自动 ROLLBACK，不留孤儿数据；全量串行跑
（同时跑两个 `cargo test` 会因共享种子行互相干扰）。

```bash
cargo fmt --check && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)
cargo test              # 需本地 MySQL（docker compose up -d mysql 后即可）
```

CI（`.github/workflows/ci.yml`，`push/PR → main` 触发）：lint job
（fmt + clippy `-D warnings` 双 crate）与 test job（MySQL service container 跑全量真库测试）。

当前全仓 522 个 `#[test]`（均在 `src/`，migrations 与 codegen 无测试），全部连真库、无 mock。

拆出 lib target 后可用 `cargo test --lib` 只跑库侧测试（改造前该命令报
`no library targets found`，因为本仓曾是 binary-only crate）。
条数会随切片增长，改代码时请同步这里。

## 🤝 贡献

欢迎 Issue 与 PR：提交信息遵循 Conventional Commits（中文描述），PR 需附
`cargo test` 结果；契约变更需同步说明响应体与端点，并更新前端对接说明。

完整流程与本地开发环境（MySQL 3307、真库测试、双 crate 门禁）见
[CONTRIBUTING.md](CONTRIBUTING.md)；安全问题请勿开公开 Issue，见 [SECURITY.md](SECURITY.md)。

## 🙏 致谢

- [Salvo](https://github.com/salvo-rs/salvo) / [SeaORM](https://github.com/SeaQL/sea-orm) — 后端框架
- [Vue Vben Admin](https://github.com/vbenjs/vue-vben-admin) — 配套前端
  [tide-admin](https://github.com/xqh-jason/tide-admin) 的脚手架基座（该仓库为 Vue Vben Admin 的二次开发）

## 📄 License

本项目基于 [MIT](LICENSE) 协议开源；配套前端 tide-admin 同为 MIT，其内部
`packages/` / `internal/` / `scripts/` 保留上游 Vue Vben Admin 的版权声明。

