<div align="center">

# tide-server

基于 **Rust · Salvo · SeaORM** 的 RBAC 中后台管理系统后端，
配套前端 [tide-admin](https://github.com/xqh-jason/tide-admin)（Vue Vben Admin 5.x）

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
- 过期凭证每日定时物理清理（保留 30 天供审计）

**权限（RBAC 三级同源）**

- 用户 / 角色 / 菜单 / API 四大管理，接口·菜单·按钮权限码同源
- 双通道鉴权：认证中间件（会话合并查询，禁用/软删用户即时 401）+
  接口级授权（`sys_api` path+method 精确匹配 + 角色绑定，未登记放行、超管短路）

**系统管理**

- 部门（树表）/ 职位 / 数据字典（类型 + 字典项）/ 参数配置 / 文件上传 / 定时任务
- 审计盖章（`created_by` / `updated_by`）与操作人名称批量拼装，请求体脱敏截断落操作日志
- 主表软删除、关系表硬删除约定贯穿全部查询

**工程化**

- 统一契约：`POST + JSON`，响应 `{ code, data, message }`（`code=1` 成功），
  HTTP 恒 200，仅认证失败 401；Swagger UI 开箱可用
- SeaORM 迁移（独立 crate）+ 启动幂等种子（菜单树 / API 权限点 85 条 / 默认定时任务）
- 438 个真库集成测试（事务回滚隔离，无孤儿数据）；clippy 对 unwrap / expect / todo /
  unsafe 全量 deny

## 🧱 技术栈

| 层 | 选型 |
|---|---|
| Web 框架 | Salvo 0.95（oapi OpenAPI 契约） |
| ORM / 迁移 | SeaORM 1.x / sea-orm-migration（独立 crate `migration`） |
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
cp .env.example .env      # 按需修改密码与 JWT secret
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

### 环境变量（`TIDE_` 前缀 + `__` 层级分隔）

| 环境变量 | 对应配置 | 说明 |
|---|---|---|
| `TIDE_ENV` | `env` | `development` / `production`（生产不重置 admin 弱口令） |
| `TIDE_SEED__ENABLED` | `seed.enabled` | 生产环境显式开启启动种子（首次部署 bootstrap 用） |
| `TIDE_DATABASE__URL` | `database.url` | 连接串，务必带 `charset=utf8mb4&timezone=%2B08:00` |
| `TIDE_JWT__SECRET` | `jwt.secret` | JWT 签名密钥，生产替换为强随机串 |
| `TIDE_JWT__TTL_SECONDS` | `jwt.ttl_seconds` | access token 过期秒数（自动识别数字类型） |
| `TIDE_JWT__REFRESH_TTL_SECONDS` | `jwt.refresh_ttl_seconds` | 刷新凭证有效期秒数 |
| `TIDE_CORS__ALLOW_ORIGINS` | `cors.allow_origins` | 跨域白名单，逗号分隔（如 `a.com,b.com`） |
| `TIDE_UPLOAD__DIR` | `upload.dir` | 上传落盘目录 |

列表类字段统一逗号分隔；同源反代部署下无需配置 CORS。

## 📡 接口契约

- 所有端点 `POST + JSON body`（文件上传为 multipart），响应体统一
  `{ code: 1, data, message }`（`code=1` 成功 / `0` 失败），HTTP 恒 200，仅认证失败 401
- 分页请求 `{ page, pageSize }`，响应 `{ total, totalPages, items }`
- Swagger UI：`/swagger-ui`（规范文件 `/api-doc/openapi.json`）

## 📁 目录结构

```
src/
├── modules/<域>/{api,service,repo,dto}.rs   # 业务垂直切片（契约驱动四件套）
├── infra/                                    # 启动管线、配置、AppState、路由登记表
├── middleware/                               # AuthRequired / OperationLog / ApiPermission
├── entity/                                   # SeaORM 实体（全局共享）
├── utils/                                    # 错误、响应体、JWT、密码、缓存、分页、人名字段拼装
└── task/                                     # 定时清理任务（job 域注册）
migrations/          # sea-orm-migration（独立 crate）
codegen/             # 代码生成器（entity / 四件套骨架）
docker/              # 容器入口脚本
```

## 🧪 测试与 CI

集成测试内联在各域 `repo.rs` / `service.rs`，直连本地 MySQL；测试夹具用事务回滚
隔离（`test_txn()`），结束（含 panic）自动 ROLLBACK，不留孤儿数据。

```bash
cargo fmt --check && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)
cargo test              # 需本地 MySQL（docker compose up -d mysql 后即可）
```

CI（`.github/workflows/ci.yml`，`push/PR → main` 触发）：lint job
（fmt + clippy `-D warnings` 双 crate）与 test job（MySQL service container 跑全量真库测试）。

## 🤝 贡献

欢迎 Issue 与 PR：提交信息遵循 Conventional Commits（中文描述），PR 需附
`cargo test` 结果；契约变更需同步说明响应体与端点，并更新前端对接说明。

## 🙏 致谢

- [Salvo](https://github.com/salvo-rs/salvo) / [SeaORM](https://github.com/SeaQL/sea-orm) — 后端框架
- [Vue Vben Admin](https://github.com/vbenjs/vue-vben-admin) — 配套前端脚手架

## 📄 License

本项目基于 [MIT](LICENSE) 协议开源；配套前端 tide-admin 同为 MIT。

