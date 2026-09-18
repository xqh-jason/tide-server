## OVERVIEW

`src/modules/` — 垂直切片容器层：`system/`（18 个平台能力域，随脚手架交付、保持稳定）+ `biz/`（业务域容器，当前为空），全部出口收敛到本目录 `mod.rs` 的 `DOMAINS` 登记表。

**建档理由**：得分 17（96 文件 / 20 子目录 / 路由登记表是全仓唯一装配点），与 `src/` 的入口职责明显分层。

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| 平台能力域（RBAC / 认证 / 字典 / 日志 / 任务 / 文件 / 部门 / 职位） | `system/<域>/` | 18 切片逐个的角色与硬规则见 `system/AGENTS.md` |
| 新业务功能 | `biz/<域>/` | 目前为空；四件套结构与 URL 契约与平台域完全一致 |
| 新增业务域（**本仓内**） | `mod.rs` 的 `DOMAINS` 追加一行 | 基座内置域走这条 |
| 新增业务域（**业务仓**） | 自己的 `main.rs` 里写 `const MY_DOMAINS` → `app::run_with_domains` | 2026-09-18 起支持；见 `modules::all_domains` 文档 |
| 挂载 / 鉴权档位 / 新增域装配 | `mod.rs` 的 `DOMAINS` + `DomainMount` + `MountGuard` + `all_domains` | `infra/router.rs::build_with` 循环消费，是**消费函数**（不再是「唯一消费者」） |
| 容器级数据有效性、权限码语义、按层命名表 | `mod.rs` 文件头 doc 注释 | 与代码同处一文件，改约定先改这里 |

## CONVENTIONS

- 一行 `DomainMount` = `path` 前缀 + `guard` + `routers` 函数数组；`infra/router.rs` 据此自动挂载，不再逐域手写 `push` + 中间件。
- **两条装配路径**（2026-09-18）：① 基座自己的域 → 直接改 `DOMAINS`；② 业务仓的域 → 在业务仓写 `const MY_DOMAINS` 并传给 `app::run_with_domains`。
  合并由 `modules::all_domains(extra)` 完成（内置在前、外部在后），**不用全局可变状态**（避免「先注册否则漏挂」的隐式时序依赖与测试并行随机失败）。
  不要写「`infra/router.rs` 是 DOMAINS 唯一消费者」——它不是了。
- `path` 空串 = 出口自带前缀（`health::routes()` 自带 `/health`、`auth::routes()` 自带 `/auth`），直接挂 `api/v1`。
- 同一前缀可并挂多个出口：`user` 行同时挂 `system::user::routes` 与 `system::menu::user_routes`（`POST /api/v1/user/menus` 业务在 menu 域）。
- `MountGuard::Protected` = 固定顺序三件套 `AuthRequired → OperationLog → ApiPermission`（顺序有意：授权失败的写操作仍要留操作日志）；`Public` = 三件套都不挂。
- `Public` 档位的域可自挂中间件：`config::site_routes` 在 `/update` 子路由自挂三件套，`auth` 的 `/logout` 自挂 `AuthRequired` + `OperationLog`。
- 新增业务域三步装配：`src/entity/mod.rs` 加 `pub mod <表>;` → 容器 `mod.rs`（biz 或 system）加 `pub mod <域>;` → `DOMAINS` 追加一行。
- 每域四件套 `{api, service, repo, dto}`，按需裁剪；校验独立成私有 `mod validate;`（不导出）。
- `entity/` 保持全局独立于本目录：关联表跨域共享（如 `sys_user_role`）。
- biz 域沿用 system 的全部工程约定（分层与 repo 边界、加锁读、软删除、按层命名与函数顺序、测试规范）——细则见 `system/AGENTS.md`，本文件不重复。

## ANTI-PATTERNS

- 不绕过 `DOMAINS` 手写路由挂载或中间件三件套（装配点收敛的意义就在于此）。
- 后端授权只认接口通道：`ApiPermission` 按 `sys_api` + `sys_role_api` 判定（内核 `permission::service::has_api_permission`）；`sys_menu.permission` 走 `/access-codes` 只控按钮显隐，判定面不消费（原 service 层按钮码校验 2026-09-17 连根删除，勿重建）。
- biz 域不得绕过 service 直接操作他域 Entity 的业务逻辑。
- 不把 `validate` 声明为 `pub mod`：它是域内私有的纯函数校验层。
