# W7 工程化部署实现计划

> **面向 AI 代理的工作者：** 本计划遵循协作约定——AI 交付配置、Dockerfile、CI 与
> 冒烟验证；用户手写涉代码改动（任务 2）。步骤中用 **[AI]** / **[用户]** 标注执行者。
> 上游计划：`docs/Rust学习计划-Salvo-vben.md` W7（表单生成器已取消）。

> **执行状态（2026-09-09，全部由 AI 代劳）：** 任务 1–7 已完成；验证结果为
> fmt / clippy / `cargo test` 307 全绿。**容器冒烟受阻**：本机无法连接 Docker Hub
>（registry-1.docker.io EOF，本地仅缓存 mysql:8），`docker compose up -d --build`
> 待网络就绪后由用户补跑（验收命令见任务 5/7）。
> 执行中额外收敛两处：① `operation_log` 用户半成品改动（ip/时间范围过滤）编译
> 修复并随全量测试通过；② 新增 `seed.enabled` 启动种子开关（生产 bootstrap，见任务 5）。

**目标：** clippy 清零 + GitHub Actions CI + 后端/前端 Dockerfile + docker-compose
一键起前后端（含 MySQL）+ README。
**技术栈：** Docker 多阶段构建、nginx 反代、GitHub Actions（MySQL service container）。

---

## 现状盘点（2026-09-09 探索结论）

- **后端**：crate `salvo-vben-admin`（edition 2024，需 Rust ≥ 1.85）；migrations 为
  独立 crate `migration`（自带 main.rs binary）；无 rust-toolchain.toml
- **配置**：`Config::load()`（`src/infra/config.rs:123`）仅读 cwd 的 `config.toml`，
  **无环境变量覆盖、无 local 覆盖**——容器化需补 env source（任务 2）
- **测试**：各域 `test_db()/test_txn()` 均走 `Config::load()` 拿连接串 →
  任务 2 的 env 覆盖同时解决 CI 测试配置问题（CI 设 `SVB_DATABASE__URL` 指向
  service container 的 3306 即可，无需改测试代码）
- **种子**：`ensure_seed` 幂等（基础菜单/角色/超管均插）；仅 `env=development`
  才执行 admin 弱口令重置等开发种子 → 生产容器设 `SVB_ENV=production`，
  基础种子仍可用
- **compose**：现仅 MySQL 服务（3307:3306，volume mysql_data），无前后端
- **前端**（`../salvo-vben-web`，独立仓库）：pnpm@11.16.0 monorepo（turbo），
  node engines `^22.18.0 || ^24.12.0`；应用 `apps/web-ele`，`pnpm build` 产出
  `dist/`；`.env.production` 已是 `VITE_GLOB_API_URL=/api/v1`（相对路径）+
  hash 路由 → nginx 只需反代 `/api`，**无 SPA history fallback 负担**；
  开发 proxy 目标 `http://127.0.0.1:8080`；无自带 Dockerfile
- **CI**：无 `.github/`；仓库托管 GitHub → GitHub Actions
- **上传**：`uploads/` 已 gitignore，运行时目录 → 容器 volume

---

## 任务 1：clippy 接入与清零（[AI]）

- [ ] `Cargo.toml` 加 `[lints.rust]`（`unsafe_code = "forbid"`）与
  `[lints.clippy]` 基线（`unwrap_used = "deny"`、`dbg_macro = "deny"`、
  `todo = "deny"`；如与既有代码冲突，允许 `allow` 例外逐条注明理由）
- [ ] `cargo clippy --all-targets -- -D warnings` 清零（双 crate：
  根 crate 与 `migrations/` 各跑一次）
- [ ] `cargo fmt --check` 保持通过

## 任务 2：配置环境变量覆盖（[AI] 失败测试 → [用户] 实现）

`src/infra/config.rs` `Config::load()` 追加 env source（config-rs 自带）：

```rust
.add_source(
    config::Environment::with_prefix("SVB")
        .separator("__")
        .try_parsing(true),   // 让列表/布尔/数字能解析
)
```

- 映射规则：`SVB_ENV` → `env`、`SVB_DATABASE__URL` → `database.url`、
  `SVB_JWT__SECRET` → `jwt.secret`、`SVB_CORS__ALLOW_ORIGINS` →
  `cors.allow_origins`（`try_parsing(true)` 下逗号分隔成数组）等
- [ ] **[AI]** 失败测试（config.rs 单元测试，`std::env::set_var` 需注意
  并行测试竞态——用 `serial_test` 或合并为一个测试函数）：
  - `load_overrides_database_url_from_env`
  - `load_overrides_jwt_secret_and_env_from_env`
  - `load_parses_cors_origins_list_from_env`
- [ ] **[用户]** 实现 → **[AI]** 复跑 `cargo test config` 转绿 + 全量无回归
- [ ] 文档注释同步：`Config` doc 注明 env 覆盖规则（SVB_ 前缀、`__` 分隔）

## 任务 3：后端 Dockerfile + .dockerignore（[AI]）

- [ ] `.dockerignore`：`target/ uploads/ graphify-out/ docs/ .git/ .codebuddy/
  .codex/ skills/ *.md config.local.toml`
- [ ] `Dockerfile` 多阶段：
  - **builder**：`rust:1-slim`（edition 2024 需 ≥ 1.85，锁 tag 时按当前
    stable 选），`cargo build --release -p salvo-vben-admin -p migration`
    （两个 binary：`salvo-vben-admin` + `migration`）
  - **runtime**：`debian:bookworm-slim`，安装 `ca-certificates tzdata curl`
    （curl 供 healthcheck），`ENV TZ=Asia/Shanghai`
    `ENV SVB_ENV=production`；拷贝两个 binary + `config.toml`（容器内默认值，
    关键项由 env 覆盖）+ `docker/entrypoint.sh`
  - **entrypoint.sh**：默认先跑 `migration`（`RUN_MIGRATIONS=0` 可跳过，
    database url 取 `SVB_DATABASE__URL`）再 `exec salvo-vben-admin`；
    `mkdir -p uploads`
  - 暴露 `8080`
- [ ] 验证：`docker build -t svb-backend .` 成功，镜像内 binary 可执行
  （`--version` 之类冒烟或直接 compose 验证，见任务 5）

## 任务 4：前端 Dockerfile（[AI]，在前端仓库实施）

> 文件落在 `../salvo-vben-web` 仓库根；本计划给出内容，实施时在前端仓库
> commit（或经用户确认由本仓库代管 build context，二选一，倾向前者）。

- [ ] `Dockerfile`（前端仓库根）：
  - **builder**：`node:22-alpine`，`corepack enable`（pnpm@11.16.0 由
    packageManager 字段钉住），`pnpm install --frozen-lockfile`，
    `pnpm build`（turbo 只构建 web-ele 及其依赖，产出 `apps/web-ele/dist`）
  - **runtime**：`nginx:alpine`，拷贝 dist → `/usr/share/nginx/html`
  - `docker/nginx.conf`（前端仓库）：
    - `location /api/ { proxy_pass http://backend:8080; }`（`client_max_body_size 12m`
      对齐后端 upload 上限 10m 留余量；`proxy_set_header Host/X-Real-IP`）
    - `location / { try_files $uri $uri/ /index.html; }`（hash 路由下仅兜底）
    - gzip on（js/css/json）
- [ ] 验证：build 成功 + nginx 配置语法检查（`nginx -t`）

## 任务 5：docker-compose 一键编排（[AI]）

扩展现有 `docker-compose.yml`（MySQL 服务保留，`3307:3306` 端口映射保留供本机开发）：

- [ ] `backend`：`build: .`，`environment`：`SVB_ENV=production`、
  `SVB_DATABASE__URL=mysql://root:${MYSQL_ROOT_PASSWORD:-root}@mysql:3306/salvo_vben?charset=utf8mb4&timezone=%2B08:00`、
  `SVB_JWT__SECRET=${SVB_JWT__SECRET:-dev-secret-change-me}`；
  `volumes: backend_uploads:/app/uploads`；`depends_on` mysql
  `condition: service_healthy`；healthcheck：`curl -fsS http://localhost:8080/api/v1/health`
- [ ] `frontend`：`build: ../salvo-vben-web`（若任务 4 选前端仓库实施，
  compose 侧改 `build.context` 指过去即可），`ports: "80:80"`，
  `depends_on: backend`（若 nginx.conf 写死 `backend` 主机名，需同 network）
- [ ] mysql 加 `healthcheck`（`mysqladmin ping -h localhost -proot`）；
  `.env.example` 列出可覆盖变量（JWT secret、MySQL 密码）
- [ ] 冒烟：`docker compose up -d --build` →
  `GET http://localhost/api/v1/health` ok、浏览器 `http://localhost` 可登录
  （超管账号来自幂等种子）、上传一张图片落 volume、`/api` 经 nginx 反代可达
- [ ] CORS 说明：同源反代下浏览器不触发跨域，`cors.allow_origins` 仅在
  前后端分离域名部署时需配置（README 注明）

## 任务 6：GitHub Actions CI（[AI]）

- [ ] `.github/workflows/ci.yml`，push/PR 触发 main：
  - **job lint**：`cargo fmt --check` + `cargo clippy --all-targets -- -D warnings`
    （根 crate 与 migrations）；`Swatinem/rust-cache` 缓存
  - **job test**：`mysql:8` service container（root/root，库名 `salvo_vben`，
    端口 3306）+ env `SVB_DATABASE__URL=mysql://root:root@localhost:3306/salvo_vben?charset=utf8mb4&timezone=%2B08:00`
    （依赖任务 2 的 env 覆盖，测试零改动）→ `cargo test`（双 crate）
  - 可选 job build：仅 main push 时 `docker build` 验证镜像可构建
    （不推镜像，后续接 registry 再说）
- [ ] 推送后确认 Actions 全绿

## 任务 7：README + 收尾（[AI]，用户 review）

- [ ] `README.md`（本仓库）：
  - 项目简介 + 技术栈（Rust/Salvo/SeaORM + Vue vben）+ 功能覆盖说明
  - 本地开发：`docker compose up -d mysql` → 迁移 → `cargo run` → 前端
    `pnpm dev`（web-ele，5173/5910 端口说明）
  - 一键部署：`docker compose up -d --build`，env 变量表（SVB_* 覆盖规则）
  - 契约说明：统一响应体、`POST + JSON` 约定、OpenAPI 入口、4 个权限端点
  - 指向 `docs/`（学习计划、模块计划索引）
- [ ] 全量验证：`cargo fmt --check` / `cargo clippy -D warnings`（双 crate）/
  `cargo test` 全绿 / CI 绿 / compose 冒烟通过
- [ ] Commit：`feat(deploy): 工程化部署（clippy 基线、CI、Docker 多阶段、compose 一键起、README，W7）` + 打卡

---

## 实现提示（避免踩坑）

- **config-rs env 解析**：`Environment::with_prefix("SVB")` 默认会把
  `SVB_DATABASE__URL` 按 `__` 拆为 `database.url`；`.try_parsing(true)` 才能
  把 `SVB_CORS__ALLOW_ORIGINS="http://a,http://b"` 解析成数组——注意
  `try_parsing` 会连 `jwt.ttl_seconds` 之类数字一起自动转，无副作用但要复跑全量测试
- **`std::env::set_var` 竞态**：Rust 2024 起是 unsafe；单测要么全部放同一个
  `#[test]` 串行执行，要么引 `serial_test`（推荐前者，少一个依赖）
- **时区**：连接串里 `timezone=%2B08:00` 必须保留（中文时间差 8 小时的老坑），
  env 覆盖的 URL 里也要带
- **migration 先于 server**：entrypoint 顺序执行即可（单机部署），不要搞
  独立 service + 重试逻辑；`RUN_MIGRATIONS=0` 留给多实例场景手动执行
- **前端 build 内存**：vben `build` 脚本已带 `--max-old-space-size=8192`，
  GitHub Actions runner / Docker builder 内存够用；若 CI OOM，给 build 步骤
  加 `NODE_OPTIONS` 调低 turbo 并发
- **镜像 tag**：builder 阶段锁 `rust:1.90-slim` 这类具体 tag（bookworm），
  避免大版本漂移；node 同理锁 `node:22-alpine`
- **uploads volume**：`VOLUME uploads` 或 compose 命名卷均可；宿主机备份
  语义优先 compose 命名卷
- **CI 服务容器端口**：GitHub Actions service container 直接暴露在
  `localhost:3306`（无需 ports 映射），与本地 3307 不同——这正是用
  `SVB_DATABASE__URL` 覆盖的原因，CI 不需要改 config.toml

## 验收标准

- `cargo fmt --check` / `cargo clippy -D warnings`（双 crate）0 报错；
  `cargo test` 全绿
- GitHub Actions CI 全绿（fmt + clippy + 真库测试）
- `docker compose up -d --build` 一条命令起 MySQL + 后端 + 前端；
  浏览器登录 → 菜单加载 → 文件上传可用
- README 覆盖本地开发与容器部署两条路径
