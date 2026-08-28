# Rust 学习计划：用 Salvo + vue-vben-admin（web-ele）复刻 gin-vue-admin

> 版本：v4.6（新增打卡机制 §9）｜ 日期：2026-08-25
> 适用对象：前端资深工程师、有 Rust 基础、有后端基础；每天 5–6 小时，**动手为主、按需查文档**

> **前置说明**：Rust 基础已有（所有权/借用/基本语法/写过一点代码），故不设独立基础周；`async/await` + `tokio` 是唯一的"必补概念"，在 W1 项目里边做边学，配合官方 Async Book 按需查，不做系统性阅读。全程以"动手 → 报错 → 查对应小节"的方式推进。

---

## 0. 目标与技术栈（终版）

**目标**：通过复刻 gin-vue-admin（GVA）的绝大部分功能，完整掌握 Rust 服务端开发（类型系统、所有权、异步、Web 框架、数据库、权限、并发、宏、测试、部署），并打通企业级前后端分离系统的权限闭环。

| 项 | 选型 | 说明 |
|---|---|---|
| 后端框架 | Salvo 0.95.x | 需 Rust 2024 edition / MSRV 1.94，直接用最新版（0.89.3 前有表单 OOM 漏洞 CVE-2026-33241） |
| 数据层 | **SeaORM 1.1.x** | 与 GVA 的 GORM 同属 ORM，映射最自然；底层基于 SQLx 驱动 |
| 数据库 | **MySQL 8** | 与 GVA 一致 |
| 权限 | 自研简易 RBAC（最终态）/ casbin-rs 可选 | 先吃透原理，casbin-rs 仅作加分项 |
| JWT | jsonwebtoken | 一致 |
| 配置 | config-rs | TOML |
| 日志 | tracing + tracing-subscriber | 从 W1 第一天引入 |
| 缓存 | 内存缓存（自研 Cache trait） | token 黑名单 / 验证码，无外部依赖 |
| 密码 | argon2 | |
| OpenAPI | Salvo 自带 #[endpoint]（salvo-oapi） | 无需单独接 utoipa |
| 系统监控 | sysinfo | 服务器状态监控（W6） |
| 定时任务 | tokio-cron-scheduler | 周期任务（W6） |
| 前端 | **vue-vben-admin v5 的 web-ele 子应用**（Element Plus） | Monorepo + Tailwind v4 + Shadcn 底座，`pnpm run dev:ele` |

**关键结论**：GVA 的 RBAC 数据模型（用户-角色-菜单-API）不用改；要改的是后端对前端的**输出契约**（详见第 3 节）。

---

## 1. GVA 设计模式拆解（对照要点）

1. **四层架构**：Router → Api(Handler) → Service → Model，数据单向流动。
2. **中间件链**：JWT（登录态）+ 权限（授权）+ CORS + 日志 + Recovery 兜底；Salvo 中间件即 Handler（`hoop` 挂载），与 Gin 一一对应。
3. **动态路由**：登录后返回菜单树 → 前端动态注册路由、生成侧边栏 → 按钮级权限控制。
4. **通用模块**：数据字典、操作日志、登录日志、文件上传（含断点续传）、验证码、代码生成器、服务器监控、定时任务。
5. **初始化管线**：config → 日志 → 数据库（迁移+初始数据）→ 缓存 → 权限 → 路由注册。
6. **全局状态设计（对照 GVA 3.0 数据架构）**：GVA 用全局变量 `global.GVA_DB` / `global.GVA_CACHE`（任何 Service 直接访问）；**本项目改用 `Arc<AppState>` 依赖注入（Depot）**——同一份数据，但测试时可替换注入、无全局可变状态，这是有意改进而非偏离。
7. **缓存抽象（对照 GVA_CACHE）**：GVA 3.0 的 `GVA_CACHE` 统一缓存抽象（Memory/Redis 自动选择）；本项目自研 `Cache trait`（内存实现）理念一致，W2 起按此设计。

**RBAC 闭环**：登录签发 JWT（含角色）→ 后续请求经 JWT 中间件解析 → 后端权限引擎校验"角色 × API 资源"→ 前端按菜单树渲染侧边栏、按权限码控制按钮。权限判定永远在后端，前端只做展示层。

---

## 2. 技术栈映射表（终版）

| 层次 | GVA（Go） | Rust 选型 | 备注 |
|---|---|---|---|
| Web 框架 | Gin（v1.10） | Salvo 0.95.x（features: `oapi`；状态注入用手写中间件） | 路由/中间件模型相近，中文文档 |
| 数据层 | GORM（v1.31） | SeaORM 1.1.x + MySQL 8 | Entity 派生 + 类型化查询 + Paginator 分页；底层 SQLx 驱动 |
| 权限 | Casbin（v3.10） | 自研 RBAC（最终态）/ casbin-rs 可选 | casbin-rs + sqlx-adapter 可用但示例少 |
| JWT | golang-jwt | jsonwebtoken | |
| 配置 | Viper | config-rs | |
| 日志 | Zap | tracing + tracing-subscriber | 第一天就引入 |
| 缓存 | go-redis | 内存缓存（自研） | 不引入外部缓存，黑名单/验证码用内存实现 |
| 校验 | validator | validator / garde | |
| API 文档 | swaggo | salvo-oapi（#[endpoint]） | 框架自带 |
| 密码 | bcrypt | argon2 | |
| 系统监控 | sysInfo | sysinfo | 跨平台系统信息 |
| 定时任务 | cron | tokio-cron-scheduler | 异步调度 |
| 前端 | Vue3 + Element Plus | vben v5 web-ele | accessMode='backend' |

---

## 3. 权限设计适配方案（vben v5 backend 模式 + web-ele）

### 3.1 vben v5 权限模型要点

- `preferences.ts` 设 `app.accessMode: 'backend'`；`apps/web-ele/src/router/access.ts` 的 `fetchMenuListAsync` 指向后端菜单接口。
- 权限码（accessCodes）与菜单树是**两条独立数据通道**：按钮级权限用全局扁平权限码数组，由 `getAccessCodes` 返回，`v-access:code` 指令 / `AccessControl` 组件 / `useAccess()` 消费。
- 动态路由：菜单 JSON 须符合 vben `MenuItemType` 契约，经 `generateRoutesByBackend` → `router.addRoute`。
- 超管无前端内置放行：vben 的 `useAccess` / `AccessControl` / `v-access` 都是集合交集判断，不存在 `roles: ['super']` 自动全通过的逻辑；官方只是把超管角色名约定为 `'super'`（backend-mock 即此）。后端模式下菜单与权限码完全由后端返回，超管 = 后端返回全量菜单 + 全量/超级权限码，前端不承担放行判断。

### 3.2 后端接口契约（4 个端点）

| 功能 | vben v5 期望 | 后端提供 |
|---|---|---|
| 登录 | `POST /login` → `{ token }` | `POST /api/v1/auth/login` |
| 用户信息 | `fetchUserInfo` → `{ userInfo, roles }` | `POST /api/v1/user/info`（roles：super_admin → `super`） |
| 权限码 | `getAccessCodes` → `string[]` | `POST /api/v1/user/access-codes`（菜单表按钮记录拍平；超管返回 `['super']` 或全量） |
| 菜单树 | `fetchMenuListAsync` → vben schema 树 | `POST /api/v1/user/menus`（sys_menu → vben 格式转换层） |

> **统一契约**：本项目所有请求一律 **POST + JSON body**——列表分页/过滤参数全部放 body（如 `UserListReq`），不用 query；无入参接口（user/info、access-codes、menus）body 传 `{}` 或不传。

### 3.3 响应体契约（容易踩坑）

- vben v5 官方模板（v5.2 / v5.4 的 web-ele `request.ts`）硬编码 `code === 0` 为成功；`defaultResponseInterceptor` 的默认 `successCode` 也是 0。**后端用 `code=200` 时，前端 `request.ts` 必须显式改为 `code === 200`（或 `successCode: 200`）——这是必做定制项，不是默认匹配。**
- **后端统一响应体：`{ "code": 200, "data": ..., "message": "ok" }`**，`code=200` 为成功，失败返回非 200 code + message。
- ⚠️ GVA 是 `{ code: 0, data, msg }`——code 值（0 vs 200）和 msg 字段名都不同，不能照抄；要么后端按 vben 默认返回 `code=0`，要么后端用 `code=200` 并在前端显式配置 `successCode: 200`（本项目采用后者，见 3.3 第一条）。
- 401 语义：token 过期返回 401，vben 内置 authenticateResponseInterceptor 处理登出/刷新（可选实现 refresh token 队列）。

### 3.4 sys_menu → vben 菜单 schema 字段映射

| vben v5 字段 | 类型 | sys_menu 字段 | 处理 |
|---|---|---|---|
| `path` | string | `path` | 顶级以 `/` 开头 |
| `name` | string | `name` | 必填、唯一（Vue Router 约束），keep-alive 依赖 |
| `component` | string | `component` | **存 `#/views/<相对路径>.vue`**，须能被 `import.meta.glob('#/views/**/*.vue')` 命中 |
| `meta.title` | string | `title` | 直接映射 |
| `meta.icon` | string | `icon` | **规范为 iconify 图标名**（vben 用 iconify，GVA 是 Element Plus 图标） |
| `meta.order` | number | `sort` | 一级菜单排序 |
| `meta.keepAlive` | boolean | `keep_alive` | |
| `meta.hideInMenu` | boolean | `hidden` | |
| `meta.activePath` 等 | - | 可选 | 预留 |

### 3.5 权限码同源原则

- 菜单表"按钮"类型记录的 `permission` 码（`模块:实体:动作`，如 `system:user:add`）是**唯一事实来源**：
  - 前端：汇总为 `POST /user/access-codes` 的权限码数组，`v-access` 判断按钮显隐；
  - 后端：自研 RBAC 策略中"角色 × API"的授权点与之对应，服务端强制拦截。
- 效果：用户"看得到按钮但调接口被 403"的双保险。

### 3.6 环境与前端注意事项（web-ele）

- vben v5 **强制 pnpm**（npm/yarn 直接报错）：Node ≥ 18.18、pnpm ≥ 8（corepack 启用）。
- 动态路由模式下，`component` 对应的页面文件必须真实存在于 `apps/web-ele/src/views/`——**前端 agent 需按菜单约定建好空壳组件**，否则菜单生成但页面 404。
- 上线前把菜单响应保存为**契约样本**，CI 校验：component 路径能被 glob 命中、路由 name/path 唯一。
- 回归必测 4 场景：刷新浏览器、直接访问深层 URL、退出后换账号、多租户切换（如有）。

---

## 复刻范围：GVA 功能覆盖

**结论**：覆盖 GVA 约 **90%** 功能；"多数据库"SeaORM 通过 DbBackend 枚举可切换，列为可选项。

| GVA 功能 | 覆盖 | 主要练的 Rust 技能 |
|---|---|---|
| 用户/角色/菜单/API 管理 | ✅ W3 | CRUD、事务、关联查询、树结构 |
| JWT 认证 + 黑名单 | ✅ W2 | 中间件、异步、内存缓存 |
| RBAC（自研） | ✅ W2 | trait 抽象、中间件 |
| 数据字典 | ✅ W5 | 通用 CRUD、缓存 |
| 操作日志 / 登录日志 | ✅ W5 | 中间件、tracing、异步落库 |
| 文件上传 | ✅ W5 | 文件流、类型边界 |
| 断点续传 | ✅ W6 | 分块、并发、异步 IO |
| 图形验证码 | ✅ W5 | 图像生成、内存缓存 |
| 代码生成器 | ✅ W4 | 宏 / 泛型 / 模板 / 代码生成 |
| 服务器状态监控 | ✅ W6 | sysinfo、系统编程 |
| 定时任务 | ✅ W6 | tokio-cron-scheduler、异步调度 |
| 系统配置（参数配置）管理 | ✅ W5 | 通用 CRUD |
| 部门/岗位管理 | ◐ W5 可选（GVA 核心组织架构，计划原缺口） | 树结构 CRUD |
| 数据权限（5 档数据范围） | ◐ W7+ 可选（复杂度高，按需） | 数据范围过滤 |
| 表单生成器 | ✅ W7（轻量） | 前端配置驱动 |
| 多数据库 | ◐ 可选 | SeaORM 通过 DbBackend 切换后端，比 GORM 略重但可支持 |
| AI 辅助生成（LLM → 字段 JSON → 生成器） | ◐ W7+ 可选 | reqwest / SSE 流式 / MCP 协议 |
| Swagger / 多环境配置 | ✅ W1 | #[endpoint]、config-rs |

---

## 4. 项目驱动学习计划（7 周，每天 5–6 小时）

> 学习风格：**动手为主，不看资料、需要时再查**。borrow checker / 生命周期 / trait 问题当场查对应小节。
> **唯一例外**：W1 第一天花 15–30 分钟读 SeaORM 官方 Entity 入门（搞懂 Entity / ActiveModel / Model 三件套），否则派生宏太玄学。
> **分工**：前端全部由你 + AI agent 负责（契约驱动），本计划只聚焦后端 Rust 实现；后端唯一的额外义务是提供**稳定契约**（统一响应体 + OpenAPI + 4 个权限端点），前端 agent 按契约编码。
> 代码生成器前移到 W4，**学完立刻用于 W5/W6 模块**，避免"学完即弃"。
> ⚠️ 可溢出点：W1（基建多）、W6（三扩展）偏紧，可顺延到第 8 周，不影响 W1–W3 主线收益。

> **最高原则：Rust TDD 手写闭环**。每个新行为先手写一个失败的集成/单元测试，运行确认它红；再写最小实现让它绿；绿了以后只做小步重构并保持全量测试通过。AI 只负责给出骨架、解释报错、审查实现，**不替你写业务核心代码**——这是为了把所有权、生命周期、trait、异步和 SeaORM 的卡点真正过一遍。

### W1 · Salvo 框架 + 工程基建 + SeaORM 数据层
- **目标**：路由/Handler/提取器/中间件（`hoop`）、统一响应 `{code,data,message}`、全局错误处理、`#[endpoint]` OpenAPI、tracing；SeaORM 连接池/迁移（sea-orm-migration）/Entity 派生/查询/事务/Paginator 分页。
- **关键点**：**本周定稿 4 张核心表（user/role/menu/api）+ 3 张关联表（user_role/role_menu/role_api）schema**；用 `sea-orm-cli` 生成 Entity 与迁移骨架；**动态过滤查询用 `Condition` 拼接**（列表页搜索依赖它）；**`#[endpoint]` OpenAPI 注释写清楚，作为前端 agent 的契约输出**；全局状态用状态注入中间件 + handler 参数 `depot` 获取（**handler 参数不支持 `&AppState` 自定义类型引用，只认 `Request/Depot/Response/FlowCtrl` + oapi 提取器**）；vben 环境由你自行准备。
- **交付**：CRUD 骨架 + 完整 migration + 真库集成测试（本项目从零开发，测试直接连 MySQL；MockDatabase 单测按需选用）。

### W2 · 认证与授权（RBAC）
- **目标**：JWT 签发/校验、认证中间件、argon2、token 黑名单、自研 RBAC、4 个权限契约端点、DB 集成测试。
- **决策**：自研 RBAC 为最终态，casbin-rs 仅作可选加分项。
- **关键点**：4 个契约端点与 vben 约定严格对齐（前端由你/AI agent 对接）；测试以真库集成测试为主（连 MySQL，测试数据唯一命名 + 测后清理），MockDatabase 单测按需（CI/无库环境）；**缓存用内存实现（自研 Cache trait，dashmap/moka），不引入外部依赖**。
- **交付**：登录闭环 + 受保护路由 + 权限码接口 + token 黑名单。

### W3 · 核心四模块（用户/角色/菜单/API 管理）
- **目标**：四表管理 + 初始化数据（admin/默认菜单/字典）+ 契约就绪（管理页面由你/AI agent 对接）。
- **动手骨架**：
  1. **用户域收口**：`create_user` 校验角色存在且启用 → argon2 哈希密码 → 同一事务写入 `sys_user` 和 `sys_user_role` → 支持空角色列表 → 测试覆盖重复用户名、未知角色、哈希落库、关联落库。
  2. **角色域 CRUD**：在 `modules/role/{dto.rs,repo.rs,service.rs,api.rs}` 补齐四件套；接口为 list/create/update/get/delete，update 角色时在同一事务维护 `sys_role_menu` 与 `sys_role_api` 关联；测试唯一命名并在清理时先删关联再删主表数据。
  3. **菜单域 CRUD**：保留 `/user/menus` 契约不变，新增菜单管理 CRUD；按 `parent_id` 构建树并检查 `name/path/component` 约束；按 `sys_role_menu` 过滤普通用户菜单。
  4. **API 权限域**：新增 `sys_api` CRUD；创建或更新 API 后维护 `sys_role_api` 关联；中间件按请求 path/method 匹配 API 记录并校验角色映射。
  5. **同源权限码闭环**：`/user/access-codes` 不再硬编码 super；改用「启用角色 → 启用菜单 `menu_type=3`」拍平非空 permission，并确保空集合不触发无效 SQL；菜单按钮变更后同名权限码立即控制前端按钮显隐。
  6. **种子与回归**：初始化 admin、默认菜单、示例按钮权限码、super 角色及全部 RBAC 关联；最后跑 `cargo fmt && cargo check && cargo test`，清理所有测试残留。
- **关键点**：菜单管理支持按钮权限码维护（改菜单 = 改权限，同源生效）；超管返回 `roles:['super']`；**用户-角色 / 角色-菜单多表写入用事务**；**many-to-many 关系派生（`Linked`）是 SeaORM 1.1.x 常见卡点，卡住先查官方 Relations 章节**（⚠️ **新式 entity**——`#[sea_orm::model]` + Model 内联 `#[sea_orm(has_one/has_many/many_to_many)]` 字段——是 **SeaORM 2.0** 功能，1.1.x 不支持；1.1.x 请用旧式 `DeriveEntityModel` + `DeriveRelation` + `Linked`。生成器用 `--model-extra-attributes` 或手写均可；若要新式 entity 需整体升级 SeaORM 2.0，涉及破坏性变更，慎重）；契约端点保持 OpenAPI 同步，前端由你/AI agent 按契约对接。
- **交付**：MVP 完成（登录→动态菜单→四模块管理全链路）。

### W4 · 代码生成器（学完立刻用）
- **目标**：建表 SQL/struct → 自动生成 entity + 每域四件套（api/service/repo/dto）。
- **关键点**：proc-macro 或 build script + 模板字符串；serde 反射做 Rust↔SQL↔TS 类型映射；泛型分页/查询抽象；**复用 `sea-orm-cli` 生成的 entity 输出作为模板基准，不重复造轮子**。
- **交付**：可运行生成器 CLI，**并立即用它生成 W5/W6 模块骨架**（全计划 Rust 深度最高的一步）。

### W5 · 通用模块（生成器加速）
- **目标**：操作日志、登录日志、数据字典、文件上传（本地/MinIO）、图形验证码、系统配置；可选 refresh token。
- **关键点**：每个模块仍是"entity + 迁移 + repo + service + api + 测试"六件套，生成器批量产出；前端页面由你/AI agent 实现。
- **交付**：六个通用模块 + 日志随请求自动落库。

### W6 · 扩展模块（并发 / 系统 / 调度）
- **断点续传**：分块上传、并发合并、异步 IO、分块哈希校验。
- **服务器状态监控**：sysinfo 采集 CPU/内存/磁盘/网络。
- **定时任务**：tokio-cron-scheduler 周期任务 + 任务 CRUD（用生成器产出）。
- **交付**：三模块后端完成。

### W7 · 表单生成器 + 工程化部署
- **表单生成器**（轻量，前端配置驱动）。
- **工程化**：cargo clippy/fmt、CI（构建+测试+菜单契约校验）、Docker 多阶段构建、docker-compose 一键起前后端（含 MySQL）、README。
- **交付**：全量完成，可部署项目。

### W7+ 延伸 · AI 辅助生成（可选，不进主线）
- **背景**：复刻 GVA 的 AI 生成功能。核心不是"AI 写代码"，而是 **"AI 产出字段定义 JSON，喂给 W4 的模板生成器"**——手工生成与 AI 生成走同一条管道，产出代码完全一致（GVA 的 llmAutoFunc / MCP gva_execute 即此模式）。
- **最小实现（1–2 天）**：`POST /api/v1/ai/fields`——接收自然语言 → `reqwest` 调任意 OpenAI 兼容 LLM → `serde_json` 解析字段 schema → 喂给 W4 生成器；SSE 流式返回用 `salvo_extra::sse::stream(&mut res, stream)`（不是 `Response::sse()`；`salvo_extra 0.95.2` 受 rsproxy 镜像约束，需先换源或本地 vendor）。
- **进阶（+1–2 天）**：Rust 实现 MCP server（Streamable HTTP），让 AI 编辑器（Claude Code / Cursor 等）直接调用生成器工具；生成时自动注册菜单 + API 权限（复用 W3 的权限码同源机制）；历史回滚（生成前备份文件与 DB 变更）。
- **可选子项**：UI 原型图识别（视觉模型 → 字段 JSON，对应 GVA 的 Eye API）。
- **学习价值**：reqwest / SSE / JSON 处理 / MCP 协议，深度中等——建议 W4 完成、项目跑通后再做，不占主线。

---

## 5. 工程结构（终版）

```
salvo-vben-admin/
├── config.toml             # config-rs 配置（在项目根目录，`config::File::with_name("config")` 读取）
├── migrations/             # sea-orm-migration（4 核心表 + 3 关联表 + 扩展模块表）
├── codegen/                # 代码生成器 CLI（W4）
└── src/
    ├── main.rs             # 入口（唯一留在根的启动文件）
    ├── infra/              # 基础设施：启动管线 / 配置 / 全局状态 / 路由
    │   ├── mod.rs
    │   ├── app.rs          # 初始化管线: db → state → OpenAPI → serve
    │   ├── config.rs       # config-rs
    │   ├── state.rs        # Arc 应用状态（AppState，含 db 连接）
    │   └── router.rs       # 路由组装（挂 modules/*/routes()）
    ├── middleware/         # 横切关注点：InjectState（W2 加 jwt / auth / cors / log）
    ├── modules/            # 业务域（垂直切片，每域 api/service/repo/dto 四件套）
    │   ├── system/         # 系统域：health（W6 并入监控/定时任务管理）
    │   │   └── api.rs
    │   └── user/           # 用户域示例（新增域 = 复制此目录 + 挂路由）
    │       ├── mod.rs      # 域内路由注册 routes()
    │       ├── api.rs      # Handler（对应 GVA api）
    │       ├── service.rs  # 业务层
    │       ├── repo.rs     # SeaORM 数据访问（含真库集成测试）
    │       └── dto.rs      # 请求/响应（含 vben 菜单 schema 转换层）
    ├── entity/             # SeaORM 实体（全局共享，对应 GVA model；关联表跨域共用）
    ├── task/               # 定时任务（tokio-cron-scheduler，W6；或并入 system 域）
    ├── utils/              # jwt / response{code,data,message} / crypt
    └── tests/              # 集成测试（当前测试内联在各域 repo.rs，可后续抽离）
前端（独立仓库或 apps/web-ele）：
    ├── preferences.ts      # accessMode='backend'
    ├── src/router/access.ts# fetchMenuListAsync → /user/menus
    ├── src/api/request.ts  # 响应体 code=200 契约（默认已匹配）
    └── src/views/system/   # user / role / menu / dict / log / monitor / task 页面
```

---

## 6. 避坑清单（合并多轮审核）

1. **vben 版本**：用 v5 的 web-ele 子应用（`pnpm run dev:ele`），不要用 v2 分支；强制 pnpm。
2. **Salvo 版本**：0.95.x 起基于 Rust 2024 edition / MSRV 1.94；别用 <0.89.3（OOM 漏洞）。
3. **响应体契约**：`{code:200,data,message}`，不是 GVA 的 `{code:0,data,msg}`；注意 vben 官方模板默认成功码是 0（`code === 0`），`code=200` 必须显式修改前端 request.ts，否则成功响应全部被判为错误。
4. **权限码与菜单分离**：vben 按钮权限是全局 accessCodes 数组，不挂在菜单树节点上。
5. **component 路径**：必须能被 `import.meta.glob` 命中 views 目录；页面文件要真实存在。
6. **SeaORM 迁移**：用 `sea-orm-migration` + `sea-orm-cli migrate up` 执行；新增表/字段一律走迁移文件，不要手改库。
7. **async trait**：Rust 2024 edition 原生支持 async fn in trait；Salvo 内部用 async_trait 宏，自定义 trait 为兼容可继续用。
8. **Send/Sync**：跨请求共享状态用 `Arc<T>` + 状态注入。
9. **错误处理（Salvo 0.95）**：库用 thiserror，应用层用 anyhow，别到处 unwrap。**handler 返回 `Result<Ok, Err>` 要求 Ok 与 Err 都实现 `Writer` trait**（不是 `Into<salvo::Error>`）；自定义错误类型为 `AppError` 实现 `Writer` 输出统一 JSON；启用 `anyhow` feature 时 `anyhow::Error` 自动映射 500。
10. **&str vs String**：函数签名里最容易栽的坑。
11. **测试**：从 W1 开始写，cargo test 是一等公民。
12. **vben 路由 name 唯一性**：后端菜单 name 需校验唯一，否则动态路由注册异常。
13. **断点续传**：分块带序号 + 哈希校验，合并前按序校验；并发上传要限流。
14. **sysinfo 跨平台**：不同 OS 字段/刷新方式有差异，测试固定在本机平台。
15. **定时任务与 tokio 版本对齐**：tokio-cron-scheduler 依赖的 tokio 版本要和 Salvo 一致，避免多运行时冲突。
16. **MySQL 类型映射**：SeaORM 用 chrono 的 `NaiveDateTime`/`Date`/`Time` 映射 DATETIME/DATE/TIME；布尔用 `bool`（SeaORM 已处理 TINYINT(1)）；JSON 列用 `Json` 类型。
17. **handler 参数类型限制**：`#[handler]`/`#[endpoint]` 参数只支持 `&Request`/`&Depot`/`&Response`/`&FlowCtrl`/`&ConnCtrl` 与 oapi 提取器（`PathParam`/`QueryParam`/`JsonBody` 等）；**自定义类型引用（如 `&AppState`）直接编译报错**——状态用 `depot: &mut Depot` + `depot.get_typed::<AppState>()` 获取。
18. **affix-state / oapi feature**：`salvo::affix_state::inject` 需 Cargo.toml 启用 `"affix-state"`（该 feature 依赖 `salvo_extra`，本项目 rsproxy 镜像缺 `salvo_extra 0.95.2`，故手写等价中间件 `InjectState`（`depot.insert_typed`）替代，无需启用该 feature）；OpenAPI 需 `"oapi"`（提取器从 `salvo::oapi::extract::*` 导入）；**手动 `req.param()` 取参不生成 OpenAPI 文档**，契约接口必须用提取器。
19. **Salvo 启动**：`TcpListener::new(addr).bind().await`（bind 失败直接 panic，非 Result）；`Server::new(acceptor).serve(router)`；`Service::new(router).catcher(Catcher::default().hoop(...))` 自定义错误页。
20. **MySQL 中文乱码**：`docker exec mysql` 客户端默认 `character_set_client=latin1`，插入中文会双重编码（存成 `C3A7C2AE...`）；写入/维护数据必须加 `--default-character-set=utf8mb4`。读取端 sqlx 自动 `SET NAMES utf8mb4`，无需额外配置。修复双重编码：`CONVERT(CAST(CONVERT(col USING latin1) AS BINARY) USING utf8mb4)`。
21. **Salvo 参数提取（统一 JSON body）**：本项目所有请求统一 **POST + JSON body**，handler 用 `JsonBody<T>` 提取（自动生成 OpenAPI requestBody schema）。通用分页结构 `PageQuery`（utils）保留 `ToSchema`，通过 `#[serde(flatten)]` 组合进各域请求 DTO（如 `UserListReq`）——serde 的 body 反序列化**支持嵌套 flatten**（与 query 提取不同），handler 只声明一个 `JsonBody<UserListReq>` 参数即可；分页字段统一在 PageQuery 单点定义。注：若个别接口仍需 query/path 提取，用 `ToParameters`/`PathParam`，此时不要同时 derive `Extractible`（E0119 冲突），且 `ToParameters` 不支持嵌套 flatten。

---

## 7. 按需查询速查表（动手遇到再查，非必读）

- **Rust**：Rust 语言圣经（在线查小节）、Rust Async Book、tokio 官方教程、Rustlings
- **Salvo**：salvo.rs 官方文档（中文）、GitHub salvo-rs/examples
- **SeaORM**：sea-ql.org 官方文档（中文）、`sea-orm-cli` 生成器说明、MySQL 类型映射章节
- **系统监控 / 定时任务**：sysinfo、tokio-cron-scheduler 的 README + examples
- **vben**：doc.vben.pro（后端访问控制章节）、reawing.com 中文镜像、apps/backend-mock 的菜单 mock（照抄格式）
- **GVA**：gin-vue-admin 源码当"需求文档"，逐目录对照（api/model/service/router/middleware/utils/config/initialize/task/timer）

---

## 8. 里程碑与打卡点

| 里程碑 | 判定标准 | 时间 |
|---|---|---|
| M1 | Salvo CRUD 骨架 + 完整 schema + OpenAPI 契约就绪 | 第 1 周末 |
| M2 | 登录闭环 + 权限码接口 + token 黑名单 | 第 2 周末 |
| M3 | 核心四模块后端 + 权限码闭环（MVP） | 第 3 周末 |
| M4 | 代码生成器可用 + 生成 W5/W6 骨架 | 第 4 周末 |
| M5 | 六通用模块完成 | 第 5 周末 |
| M6 | 断点续传 + 监控 + 定时任务 | 第 6 周末 |
| M7 | 表单生成器 + 部署上线（全量完成） | 第 7 周末 |

---

## 9. 打卡与进度追踪

- **打卡文件**：`Rust学习打卡记录.md`（与本计划同目录）。每天花 1 分钟记录：目标 / 完成 / 卡点 / 明日。
- **频率**：每天一记；周日对照 §8 里程碑复盘（M1–M7 打勾），看到每周的累计进步。
- **原则**：只记"解决了什么 / 卡在哪"，可沉淀回看，不写流水账。
- **可选**：可配置每日提醒自动化，到点提示打卡。
