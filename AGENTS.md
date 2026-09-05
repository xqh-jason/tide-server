# Repository Guidelines

基于 Rust + Salvo + SeaORM 的 RBAC 后端项目（复刻 gin-vue-admin，前端为 vue-vben-admin）。
业务按垂直切片组织，契约驱动：所有接口统一 `POST + JSON body`，响应体
`{ code: 200, data, message }`。

## 项目结构与模块组织

- `src/modules/<域>/{api,service,repo,dto}.rs`：业务垂直切片（如 user、role、menu、permission）
- `src/infra/`：启动管线、配置、全局状态（`AppState`）、路由组装
- `src/middleware/`：横切中间件（`InjectState`、`AuthRequired`）
- `src/entity/`：SeaORM 实体，全局共享
- `src/utils/`：错误、响应体、JWT、密码、缓存、分页
- `migrations/`：sea-orm-migration 迁移（独立 crate）
- `docs/`：学习计划与实现计划（`docs/superpowers/plans/` 按 TDD 勾选推进）

## 构建、测试与运行

- `cargo fmt --check`：格式校验（rustfmt）
- `cargo check`：类型检查
- `cargo test`：全量测试（需本地 MySQL：`docker compose up -d`）
- 迁移：在 `migrations/` 目录执行
  `DATABASE_URL='mysql://root:root@localhost:3307/salvo_vben' cargo run -- up`
- 启动：`cargo run`（监听 0.0.0.0:8080；注意根目录的 `cargo run -- up` 会启动服务器而非迁移）

## 编码风格与命名

- 注释用简体中文，中英文之间留空格；标识符用英文命名
- 域内四件套职责：api（handler）/ service（业务规则）/ repo（SeaORM 数据访问）/ dto（传输对象）
- handler 返回 `ApiResult<T>`；业务错误用 `AppError`；repo 层一律 `?` 传播错误，禁止 `unwrap`
- 软删除约定：主表查询默认过滤 `deleted_at IS NULL`，关系表（`sys_role_menu` 等）硬删除
- 权限码、超管角色键等魔法字符串集中为常量（`SYSTEM_USER_CREATE`、`SUPER_ROLE_KEY`）
- 接口命名按层统一：repo 分页用 `find_page`；service 分页用 `page_<实体>`（如 `page_users`）；
  handler 端点用 `list_<实体>`；CRUD 函数统一 `create_/update_/get_/delete_<实体>`
- 同一域内 `api.rs` 函数顺序 = `mod.rs` 路由挂载顺序 = `list → create → update → get → delete`，
  特殊契约端点（`info`、`access-codes`、`menus` 等）排在 CRUD 之后

## 人字段命名与名称拼装约定

凡指向 `sys_user` 的引用字段（创建人、更新人、审批人、申请人等）统一遵循：

- **字段命名**：人字段一律 `动词过去式_by`（`created_by`、`approved_by`、`submitted_by`、
  `assigned_by`），不与 `xxx_id` 混用；列类型 `BIGINT UNSIGNED NOT NULL DEFAULT 0`
  （`0` = 种子/系统写入），实体与 Resp 中均为 `u64` 非 Option。
- **名称字段**：Resp 中对应字段 = 人字段名 + `_name`（`created_by_name`、`approved_by_name`），
  由后端批量拼装返回，前端不做 id → 名称换算；查不到显示名时为空串，由前端渲染占位符。
- **拼装协议**（单一实现，放 `src/utils/`，只依赖 entity、不依赖任何 module）：
  - 实体实现 `UserRefIds::user_ref_ids()`：收集本记录全部人字段 id（`vec![self.created_by, ...]`）；
  - Resp 实现 `UserRefNames::set_user_ref_names()`：有几个人字段填几个 `*_name`；
  - service 端点统一 `fill_user_names(items, db, Resp::from)` 一行拼装（收集 → 一次
    `IN` 批量查显示名 → 填充；显示名取 `username`；不排除软删——名称解析面向
    历史引用，操作人即便已软删，历史记录仍应带出名字）。
  - 新增人字段 = 实体 impl 加一个 id + Resp 加一对字段，协议与管道零改动。
- **写入口径**：审计字段由 repo 层统一盖章（create 双写 `created_by`/`updated_by`、
  update 只刷 `updated_by`），service 只透传 `actor_id`；请求体不接受人字段，防伪造。
- 现状：`created_by` / `updated_by` 已落地；名称拼装协议待实现，实现后本节为唯一事实来源。

## 测试规范

- 集成测试内联在各域 `repo.rs` / `service.rs`，直连 MySQL；纯单元测试在 `utils/`
- 测试数据唯一命名（`<prefix>_<pid>_<seq>`），测后清理：先删关联表，再删主表
- 命名格式：`<行为>_<场景>`，如 `find_page_filters_by_keyword_and_status_excludes_deleted`
- 运行单个模块：`cargo test <模块名>`（如 `cargo test role`）

## 提交与 PR 规范

- Conventional Commits + 中文描述，如
  `feat(rbac): 完善软删除过滤与用户创建校验`、`chore(db): 补充 RBAC 表和字段注释`
- 类型：`feat` / `fix` / `refactor` / `chore` / `docs` / `test`
- PR：说明改动目的、附验证证据（`cargo test` 结果）、契约变更需同步说明响应体与端点

## 协作约定

- W3 起固定分工：AI 编写失败测试并做最终 review，用户手动实现业务代码
- 新功能先建计划文档（`docs/superpowers/plans/`），按测试红 → 实现 → 验证推进
- 测试与迁移需要连接本地 MySQL，在非沙箱环境执行
