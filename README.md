# tide-server

基于 **Rust + Salvo + SeaORM** 的 RBAC 后台管理系统后端，复刻 [gin-vue-admin](https://github.com/flipped-aurora/gin-vue-admin)，前端使用 [vue-vben-admin](https://github.com/vbenjs/vue-vben-admin)（独立仓库 `tide-admin`，应用为 `web-ele`）。

## 技术栈

| 层 | 选型 |
|---|---|
| Web 框架 | Salvo 0.95（oapi OpenAPI 契约） |
| ORM / 迁移 | SeaORM 1.x / sea-orm-migration（独立 crate `migration`） |
| 认证 | JWT（jsonwebtoken）+ 自研 RBAC（接口/菜单/按钮三级权限码同源） |
| 配置 | config-rs（`config.toml` + `TIDE_` 环境变量覆盖） |
| 数据库 | MySQL 8（utf8mb4） |
| 部署 | Docker 多阶段构建 + docker-compose + GitHub Actions |

## 功能覆盖

已完成：用户/角色/菜单/API 管理、RBAC 权限码、JWT 认证、数据字典、操作日志、
登录日志、文件上传、图形验证码、系统配置、定时任务（job + 日志清理）、CORS。
明确不做（见 `docs/Rust学习计划-Salvo-vben.md`）：断点续传、服务器监控、表单生成器。

## 目录结构

```
src/
├── modules/<域>/{api,service,repo,dto}.rs   # 业务垂直切片（垂直切片 + 契约驱动）
├── infra/                                    # 启动管线、配置、AppState、路由登记表
├── middleware/                               # AuthRequired / OperationLog / ApiPermission
├── entity/                                   # SeaORM 实体（全局共享）
├── utils/                                    # 错误、响应体、JWT、密码、缓存、分页、人名字段拼装
└── task/                                     # 定时清理任务（job 域注册）
migrations/          # sea-orm-migration（独立 crate）
codegen/             # 代码生成器（entity / 四件套骨架）
docs/                # 学习计划与各模块实现计划（docs/superpowers/plans/）
```

接口契约约定：所有端点 `POST + JSON body`（文件上传为 multipart），响应体统一
`{ code: 200, data, message }`；Swagger UI 在 `/swagger-ui`（规范文件 `/api-doc/openapi.json`）。

## 本地开发

前置：Rust 1.96+、Docker（仅起 MySQL 用）、pnpm（前端）。

```bash
# 1. 起开发 MySQL（3307 映射到宿主机；首次会自动建库）
docker compose up -d mysql

# 2. 建表（在 migrations 目录）
cd migrations
DATABASE_URL='mysql://root:root@localhost:3307/tide_server' cargo run -- up
cd ..

# 3. 启动后端（监听 0.0.0.0:8080，development 环境自动执行幂等种子）
cargo run

# 4. 前端（tide-admin 仓库）
cd ../tide-admin
pnpm install
pnpm dev --filter=@vben/web-ele     # dev 代理 /api → http://127.0.0.1:8080
```

## 一键部署（docker compose）

前后端仓库需同级存放：`tide-server/` 与 `tide-admin/`。

```bash
cp .env.example .env      # 按需修改密码与 JWT secret
docker compose up -d --build
```

- 前端：http://localhost （nginx 静态托管 + `/api` 反代后端）
- 健康检查：http://localhost/api/v1/health
- 后端容器启动时自动执行迁移；上传文件落在 `backend_uploads` 卷

### 首次部署 bootstrap（创建初始 admin）

生产模式默认不执行种子（避免弱口令重置）。首次部署需显式开启一次：

```bash
TIDE_SEED_ENABLED=true docker compose up -d --build backend   # 建初始 admin / super 角色
docker compose up -d backend                                  # 改回默认后重启（不再重置密码）
```

随后**立即登录并修改 admin 密码**（初始凭据 `admin / admin123`，与开发环境约定一致）。

### 环境变量（`TIDE_` 前缀 + `__` 层级分隔）

生产部署可直接用环境变量覆盖 `config.toml`（优先级更高），无需改配置文件：

| 环境变量 | 对应配置 | 说明 |
|---|---|---|
| `TIDE_ENV` | `env` | `development` / `production`（生产不重置 admin 弱口令） |
| `TIDE_SEED__ENABLED` | `seed.enabled` | 生产环境显式开启启动种子（首次部署 bootstrap 用） |
| `TIDE_DATABASE__URL` | `database.url` | 连接串，务必带 `charset=utf8mb4&timezone=%2B08:00` |
| `TIDE_JWT__SECRET` | `jwt.secret` | JWT 签名密钥，生产替换为强随机串 |
| `TIDE_JWT__TTL_SECONDS` | `jwt.ttl_seconds` | 过期秒数（自动识别数字类型） |
| `TIDE_CORS__ALLOW_ORIGINS` | `cors.allow_origins` | 跨域白名单，逗号分隔（如 `a.com,b.com`） |
| `TIDE_UPLOAD__DIR` | `upload.dir` | 上传落盘目录 |

列表类字段统一逗号分隔；同源反代部署下无需配置 CORS。

## CI

`.github/workflows/ci.yml`：`push/PR → main` 触发。两个 job：
- **lint**：`cargo fmt --check` + `cargo clippy --all-targets -D warnings`（双 crate）
- **test**：MySQL service container + 环境变量覆盖数据库地址，跑全量真库集成测试

本地等价验证：

```bash
cargo fmt --check && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)
cargo test              # 需本地 MySQL（docker compose up -d mysql 后即可）
```

## 相关文档

- 学习与功能规划：`docs/Rust学习计划-Salvo-vben.md`
- 各模块实现计划与设计：`docs/superpowers/plans/`、`docs/superpowers/specs/`
