<div align="center">

# tide-server

基于 **Rust · Salvo · SeaORM** 的 RBAC 中后台管理系统后端，配套前端
[tide-admin](https://github.com/xqh-jason/tide-admin)（基座为 Vue Vben Admin 5.x）

[![CI](https://github.com/xqh-jason/tide-server/actions/workflows/ci.yml/badge.svg)](https://github.com/xqh-jason/tide-server/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![rust](https://img.shields.io/badge/rust-1.96.0-orange.svg)](./rust-toolchain.toml)
[![salvo](https://img.shields.io/badge/salvo-0.95-red.svg)](https://github.com/salvo-rs/salvo)
[![sea-orm](https://img.shields.io/badge/sea--orm-1.x-green.svg)](https://github.com/SeaQL/sea-orm)
[![mysql](https://img.shields.io/badge/mysql-8-4479a1.svg)](https://www.mysql.com/)

</div>

## ✨ 功能特性

- **认证与会话**：账号密码 + 图形验证码登录，失败提示统一为「用户名或密码错误」防账号枚举，
  失败原因分级落登录日志；双凭证（短时效 access JWT + 长时效 refresh token，后者只进 HttpOnly
  Cookie、库中只存 SHA-256 哈希，时效见 [`config.toml`](config.toml) 的 `[jwt]`）；
  `POST /auth/refresh` 静默续期并重查角色，权限变更最迟一个 token 周期生效；登出 / 强制下线
  即时生效、重启不丢；过期凭证每日物理清理（保留天数可配，`0` = 永久）
- **权限（RBAC）**：用户 / 角色 / 菜单 / API 四大管理，菜单、接口权限点与按钮权限码统一在种子登记
  （按钮码只控前端显隐）；两级鉴权 —— 认证中间件（禁用 / 软删用户即时 401）+
  接口级授权（路径规范化后按 `sys_api` 的 path + method 精确匹配角色绑定，超管短路，未登记放行）
- **系统管理**：部门（树）/ 职位 / 字典 / 参数配置 / 网站设置 / 文件 / 定时任务 / 会话管理；
  审计字段（`created_by` / `updated_by`）由 repo 层盖章，操作人名称批量拼装；操作日志只记写请求
  （非 POST 与 `/list`、`/get` 等只读语义端点不落库），请求体脱敏截断后入库，授权失败的写请求仍留痕；
  操作 / 登录 / 调度日志与会话的保留期独立可配（`0` = 永久保留），清理任务每日物理删除；
  主表软删除、关系表硬删除
- **工程化**：统一响应体 `{ code, data, message }`（`code=1` 成功）；SeaORM 迁移
  （含列表查询复合索引）+ 启动幂等种子（菜单 / 接口权限点 / 定时任务）；分页统一按 `id` 降序；
  Swagger UI 开箱可用；真库集成测试内联在各域（事务回滚隔离）；
  clippy 对 unwrap / expect / todo / unsafe 全量 deny

## 🖼 界面预览

| 登录页 | 角色管理 | 菜单管理 |
| :---: | :---: | :---: |
| [![登录页](screenshots/01-login.png)](screenshots/01-login.png) | [![角色管理](screenshots/02-role.png)](screenshots/02-role.png) | [![菜单管理](screenshots/03-menu.png)](screenshots/03-menu.png) |
| 账号密码 + 图形验证码 | 内置 `super` 超管不可编辑 | 菜单树与按钮权限码登记 |

| 定时任务 | API 管理 |
| :---: | :---: |
| [![定时任务](screenshots/04-job.png)](screenshots/04-job.png) | [![API 管理](screenshots/05-api.png)](screenshots/05-api.png) |
| Cron 调度 + 执行日志 | 接口权限点登记（鉴权判定的数据源） |

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
| MySQL | 8.x（推荐经 docker compose 起本地实例） |
| 前端（可选） | Node `^22.18 \|\| ^24.12` + pnpm 11 |

```bash
docker compose up -d mysql        # 1. 起开发 MySQL（宿主机 3307，首次自动建库）

cd migrations                     # 2. 建表（根目录的 cargo run 是启动服务，不是迁移）
DATABASE_URL='mysql://root:root@localhost:3307/tide_server?charset=utf8mb4&timezone=%2B08:00' cargo run -- up
cd ..

cargo run                         # 3. 启动后端：0.0.0.0:8080，development 自动跑幂等种子

cd ../tide-admin                  # 4. 前端（可选，与后端同级存放）
pnpm install
pnpm dev:ele                      # http://localhost:5910，/api 代理 → 127.0.0.1:8080
```

默认账号 `admin / admin123`（development 每次启动重置；生产环境务必第一时间改密）。

## 🐳 Docker 一键部署

前后端仓库需同级存放（前端在别处时用 `FRONTEND_DIR=<你的前端目录>` 指定）。

```bash
cp .env.example .env      # TIDE_JWT_SECRET 必设（生产用强随机串），密码按需修改
docker compose up -d --build
```

- 前端 http://localhost（nginx 静态托管 + `/api` 反代后端），健康检查 http://localhost/api/v1/health
- 后端容器启动时自动执行迁移，上传文件落在 `backend_uploads` 卷

```bash
# 首次部署：显式开启一次种子（生产默认不播种，避免弱口令被重置）
TIDE_SEED_ENABLED=true docker compose up -d --build backend
docker compose up -d backend      # 改回默认后重启，随后立即登录修改 admin 密码
```

### ⚠️ 上线前必做

1. **关闭种子**：production 保持 `TIDE_SEED__ENABLED=false`（默认即关），否则每次重启都会把
   `admin` 密码重置为 `admin123`。
2. **换 JWT 密钥**：`TIDE_JWT__SECRET` 必须是强随机串；production 下仍用内置开发密钥会
   **拒绝启动**（fail-fast，有意设计）。
3. **确认上传目录与保留期**：`TIDE_UPLOAD__DIR` 不要对外静态托管；按合规要求设置
   `TIDE_LOG_RETENTION__*_DAYS`（`0` = 永久保留）。

接口鉴权对**未登记的 `path + method` 一律放行**（fail-open）：清空 `sys_api` 会让所有已登录用户
可调用全部接口。

配置项与默认值以 [`config.toml`](config.toml) 与 [`.env.example`](.env.example) 为准；
常用覆盖变量：`TIDE_ENV`、`TIDE_DATABASE__URL`（务必带 `charset=utf8mb4&timezone=%2B08:00`）、
`TIDE_JWT__SECRET`、`TIDE_JWT__TTL_SECONDS`、`TIDE_JWT__REFRESH_TTL_SECONDS`、
`TIDE_CORS__ALLOW_ORIGINS`、`TIDE_UPLOAD__DIR`、`TIDE_LOG_RETENTION__*_DAYS`
（`TIDE_` 前缀 + `__` 层级分隔，列表值逗号分隔）。

## 📡 接口契约

- 端点统一 `POST + JSON body`，响应体 `{ code: 1, data, message }`（`code=1` 成功 / `0` 失败），
  HTTP 恒 200——例外只有认证失败 401（含 `/auth/refresh`）与 CORS 预检 204/403
- `/auth/refresh` 成功时响应体为**裸 token 字符串**（非统一信封），前端在 HTTP 层直取
  `resp.data` 当新 token；失败返回真 401，前端据此走重新登录
- 契约例外：`file/upload` 为 multipart，`file/download` 与 `site-config/get` 为 GET
- 分页请求 `{ page, pageSize }`（`pageSize` 上限 1000），响应 `{ total, totalPages, items }`，
  统一按 `id` 降序；操作日志 `keyword` 为路径**前缀**匹配（如 `/api/v1/user`）
- Swagger UI：`/swagger-ui`（规范文件 `/api-doc/openapi.json`）

## 📁 目录结构

```
src/
├── lib.rs                       # 库入口：把各模块整体公开给本仓 bin target
├── main.rs                      # 进程入口：tracing + Config::load → infra::app::run
├── modules/system/<域>/         # 平台能力域（api/service/repo/dto 四件套）
├── modules/biz/<模块>/<域>/     # 业务域（按模块分组，四件套写法同平台域；本仓 hr 分支下见 hr/*）
├── infra/                       # 启动管线、Config、AppState、路由装配、种子
├── middleware/                  # InjectState / AuthRequired / ApiPermission / OperationLog / CORS / 超时
├── entity/                      # SeaORM 实体（全局共享）
├── utils/                       # 错误、响应体、JWT、密码、缓存、分页、人名字段拼装
└── task/                        # 保留期清理任务（天数读配置）
migrations/    # sea-orm-migration（独立 crate，包名 `migration`，含列表查询复合索引）
codegen/       # 代码生成器（entity / 四件套骨架）
docker/        # 容器入口脚本
```

## 🧩 扩展业务域

平台能力（认证 / RBAC / 字典 / 日志 / 任务 / 文件 / 组织）已经就绪，业务域与它并列生长：
`src/modules/biz/<模块>/<域>/`——按模块分组，四件套写法与平台域完全一致（约定见 `biz/mod.rs`
注释），实体可用 `codegen/` 生成。

接入三处：`src/entity/mod.rs` 加 `pub mod <表>;` → `src/modules/biz/mod.rs` 加
`pub mod <模块>;`（域在模块自己的 `mod.rs` 里声明）→ `src/modules/mod.rs` 的 `DOMAINS` 加一行。
登记为 `MountGuard::Protected` 即自动获得 `AuthRequired` / `OperationLog` / `ApiPermission`
三件套，不用自己接鉴权。

**新端点必须登记到 `src/infra/seed.rs` 的 `API_SEEDS`**，否则接口授权对它是 fail-open（未登记即放行）。

其余约定：业务表迁移在 `migrations/` 追加（不改已发布的 baseline，平台表相对顺序不动）；
业务数据放独立连接串指向的库（代码不假设库名，一个库里平台表与业务表并存，自己一套 admin
与 RBAC 数据）；业务表引用 `sys_user.id` 用**逻辑外键**（全库不加物理外键），审计字段由 repo
层盖章。前端页面与接口文件在 `tide-admin` 里按同样的模块分组镜像：
`apps/web-ele/src/views/biz/<模块>/<域>/` + `apps/web-ele/src/api/<模块>/<域>.ts`。

## 🏢 人事业务域（`hr` 分支）

平台与业务用**两个长期分支 + 单向合并**承载：`main` 是纯平台（开源消费方 clone 到的就是它，
不含任何业务源码），`hr` = 平台 + 人事域（本项目唯一自用部署）；业务只以**追加**方式落地，
`hr → main` 永久禁止（CI 有 `禁 hr→main 合并` 守卫 job）。人事域当前 15 张表、64 个端点：

| 域 | 表 | 端点 | 要点 |
|---|---|---|---|
| `hr/employee` | `hr_employee` | `/api/v1/hr/employee/*`（5） | 建档案可同事务建登录账号；`manager_employee_id`（直属上级）是审批「直属上级」节点的数据源，更新入参 `managerEmployeeId` 三态（缺省不修改 / `0` 清空 / `>0` 改写）；敏感字段空串 = 不修改、含 `*` 的掩码值一律拒收 |
| `hr/time-off` | `hr_time_off_type` / `_grant` / `_balance` / `_balance_log` / `_request` | `/api/v1/hr/time-off/{type,grant,balance,request}/*`（20） | 额度以**授予批次**为事实来源，FEFO 扣减、预占即扣批次；请假单「建单即提交」（时长由后端按**排班 × 工作日历**派生，请求体不接受该字段）→ 起审批实例 + 预占额度同一事务；**预占 / 释放 / 实扣流水以审批实例 ID 为来源（一次提交周期）**，`update` / `submit` 仅限「已驳回 / 已撤销」；流水 append-only，过期批次由每日任务作废 |
| `hr/approval` | `hr_approval_flow` / `_flow_node` / `_instance` / `_record` | `/api/v1/hr/approval/{flow,flow-node,instance,record}/*`（15） | 多节点顺序审批：模板（谁审）→ 实例（走到哪）→ 节点记录（每步结论）；节点类型：直属上级 / 部门负责人 / 指定用户 / 指定角色（角色池按 `sys_user_role` 展开待办，只认**启用**角色）；**最后一个节点不允许跳过**，且解析出的审批人必须账号启用（角色池要有启用成员）；终态三态分派（通过 / 驳回 / 撤销） |
| `hr/attendance` | `hr_shift` / `hr_shift_schedule` / `hr_attendance_record` / `hr_work_calendar` | `/api/v1/hr/attendance/{shift,schedule,record,calendar}/*`（16） | 公司排班制（多班次、支持跨天班）；「应出勤」由**排班 × 日历**派生、不落冗余列；打卡数据走**归一化行导入**（`source`：1 导入 / 2 手工补录 / 3 设备 / 4 钉钉 / 5 飞书），对接第三方只需把自己的响应映射成该 DTO（`externalId` 传空串按「无外部 ID」处理）；班次工时须与窗口自洽、批量排班有总行数上限 |
| `hr/overtime` | `hr_overtime_request` | `/api/v1/hr/overtime/*`（8） | 加班时长 = 区间总长（不裁剪到班次窗口，改为校验「工作日类型必须在应工作窗口之外」）；同日区间重叠把**在途（审批中）**单据一并算入；审批通过且 `comp_mode = 1 转调休` 时**同事务**生成调休额度批次（`sourceKind = 3 加班单`） |

**写入口一律「本人」**：请假单 / 加班单的 `create` / `update` / `submit` / `cancel` / `delete` 都要求单据归属 =
当前登录用户的员工档案（`create` 的 `employeeId` 必须等于本人档案 ID，加班单归属不可修改）——
HR 不做代报，批量事实录入走 `hr/attendance/record/import`；若要开放代录请加独立端点 + 独立权限码。

审批是**跨域复用**的：请假与加班各自只写自己的单据，提交时调 `hr/approval` 起实例，终态由审批域按
`biz_type` 分派回业务域（同一事务内完成实扣 / 释放 / 调休入账）。

额度账本的两条不变式（改动额度逻辑必读、必测）：

```
Σ log.delta_minutes      == granted + adjust − used − locked − expired
Σ 未失效批次 remaining + locked == granted + adjust − used − expired
```

账本口径：预占即扣批次、流水 append-only 且按 `(source_kind, source_id)` 分组结算；`source_kind = 2 请假单`
时 `source_id` 记**审批实例 ID**（= 一次提交周期）——单据允许「驳回 → 改 → 重新提交」，
用单据 ID 会让两轮预占串在一本账上（第二轮释放被幂等守卫跳过、实扣按两轮求和判「预占量不足」）。

**前端**：菜单与按钮权限码已随种子登记（`/hr` 下 4 组目录 / 页面 / 按钮码），页面本身在
[tide-admin](https://github.com/xqh-jason/tide-admin) 的 `views/biz/hr/**`，尚未补齐。

**薪酬（P5）未做**：加班费与事假 / 病假扣薪不算金额，考勤事实只落库与展示
（`overtime.comp_mode = 2 计加班费` 只落单据、`hr_time_off_type.pay_ratio` 暂为预留列）。

完整约定（分层、事务边界、账本口径、陷阱清单）见 [AGENTS.md](AGENTS.md)。

## 🧪 测试与 CI

集成测试内联在各域 `repo.rs` / `service.rs`，直连本地 MySQL，夹具用事务回滚隔离
（`test_txn()`，结束含 panic 自动 ROLLBACK）；**勿并行跑两个 `cargo test`**（共享种子行会互相干扰）。

```bash
cargo fmt --check && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)
cargo test              # 需本地 MySQL（docker compose up -d mysql 后即可）；--lib 只跑库侧测试
```

CI（`.github/workflows/ci.yml`，`push` 到 `main` / tag 与全部 PR 触发）：lint job 对两个 crate 跑
fmt + clippy `-D warnings`，test job 起一个 MySQL 8 容器跑全量真库测试；全仓无 mock。

## 🤝 贡献

欢迎 Issue 与 PR：提交信息遵循 Conventional Commits（中文描述），PR 附 `cargo test` 结果；
契约变更需同步说明端点与响应体。完整流程见 [CONTRIBUTING.md](CONTRIBUTING.md)；
安全问题请勿开公开 Issue，见 [SECURITY.md](SECURITY.md)。

## 🙏 致谢

- [Salvo](https://github.com/salvo-rs/salvo) / [SeaORM](https://github.com/SeaQL/sea-orm) —— 后端框架
- [Vue Vben Admin](https://github.com/vbenjs/vue-vben-admin) —— 前端
  [tide-admin](https://github.com/xqh-jason/tide-admin) 的脚手架基座

## 📄 License

本项目与配套前端 tide-admin 均为 [MIT](LICENSE) 协议开源；tide-admin 内部
`packages/` / `internal/` / `scripts/` 保留上游 Vue Vben Admin 的版权声明。
