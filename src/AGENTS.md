## OVERVIEW

`src/` — **lib + bin 双 target**（2026-09-18 起）：`lib.rs` 把 6 个模块整体公开，供本仓的 `main.rs` 取用。
`main.rs` 只是进程级入口（tracing + `Config::load` + `infra::app::run`）。

**建档理由**：得分 16（153 个 .rs 文件 / 26 个子目录 / 760 个顶层 pub 项 / 中心化引用极高；grep 实测，无 rust-analyzer），全仓源码唯一入口层。

## STRUCTURE

```
src/
├── lib.rs       # 库入口：6 个 pub mod（entity/infra/middleware/modules/task/utils），供 bin target 取用
├── main.rs      # 进程入口：LocalSeconds 时间戳 + RUST_LOG + Config::load → infra::app::run
├── entity/      # 有独立档
├── infra/       # 有独立档
├── middleware/  # 有独立档
├── modules/     # 有独立档（modules/system/ 再有一档）
├── task/        # 无独立档：注册表规则见下表
└── utils/       # 有独立档
```

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| 新增定时任务 | `task/<name>.rs` + `task/mod.rs` | 注册两处：`handlers()` 与 `handler_defs()` 各加一行（引用各文件 `HANDLER_NAME` / `HANDLER_LABEL` 常量，拼错即编译失败），两个漂移守卫测试会拦漏改。`HANDLER_NAME` = `sys_job.handler_name` 合法值，**不要求等于文件名**（实况：`job_log_cleanup.rs` → `cleanup_job_logs`） |
| 任务跨域取数 | `task/*_cleanup.rs` | 只调目标域 `repo` 原语（`delete_created_before` / `delete_expired_before`），不碰他域 Entity |
| 加第 7 个顶层模块 | `lib.rs` + `main.rs` | `lib.rs` 加 `pub mod`；各目录自己的 `mod.rs` 决定对外可见性 |
| 日志时间格式 / 级别 | `main.rs` | `LocalSeconds` 输出 `%Y-%m-%d %H:%M:%S`，替掉默认 UTC RFC3339 |
| 业务域、分层、锁、测试规范 | `modules/system/AGENTS.md` | 18 个平台切片的通用工程约定都在那一层 |
| 表结构 / 实体 | `entity/AGENTS.md` | 关联表跨域共享，故不归任何单一域 |
| 启动顺序、配置项、种子数据 | `infra/AGENTS.md` | |
| 请求链路上的横切逻辑 | `middleware/AGENTS.md` | |
| 错误码、响应体、分页、人字段 | `utils/AGENTS.md` | |

## CONVENTIONS

- 分层方向与跨域例外（api -> service -> repo -> entity；middleware / task / infra::seed / job::scheduler 直调目标域 repo）见 root AGENTS.md 与 modules/system/AGENTS.md
- `modules` 与 `middleware` 互相引用，仅限两件事：handler 取 `middleware::auth::AuthUser`；中间件调目标域 repo。
- `main.rs` 从 lib 取模块（`use tide_server::...`）；各目录自己的 `mod.rs` 决定对外可见性——`lib.rs` 只是把它们整体公开，**不把任何私有项变得可见**（各域 `validate` 依旧不在 `mod.rs` 里 `pub mod`）。
- 任务注册表保持显式手动：`task/mod.rs` 即全局任务索引，键引用各文件 `HANDLER_NAME` 常量，拼错即编译失败。
- 新增业务域的三步装配入口是 `modules` 模块的 `DOMAINS` 登记表（见 `modules/AGENTS.md`）；路由挂载不再需要逐域手写。

## ANTI-PATTERNS

- ~~不新建 `src/lib.rs`~~ **（2026-09-18 废止，勿再援引）**：原规则的理由是「集成测试内联在各域，不需要 lib target」，现已拆出 `lib.rs` 供 bin target 使用。
- 平台能力（RBAC / 认证 / 字典 / 日志 / 任务 / 文件 / 组织）写进 `modules/system/`；具体业务功能写进 `modules/biz/`。
- 不引入 linkme 之类宏自动注册任务（注册关系不可见 + 多一个依赖），也不做 DB 驱动扩展——handler 是代码，必须编译期注册；DB 只存「哪个任务用哪个 handler + 什么时候跑」。
- 不在 `main.rs` 之外初始化 tracing，不把日志时间戳改回 UTC。
