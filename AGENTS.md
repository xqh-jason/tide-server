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

## 分层依赖约定（跨层 / 跨域访问规则）

repo / service / task / middleware 的依赖方向与跨域访问规则：

- **repo 是数据访问原语层**：把 SQL 查询/变更原子化（过滤 / 排序 / 分页 / 审计盖章），
  不带业务判断。repo **不得调用其他域 repo 或 service 的函数**——用户角色解析等
  「跨实体查询」需要复用他域语义时，由 service 层组合后把已解析的入参传给 repo
  （如 `find_menus_by_role_ids(db, &role_ids)`，而非 repo 内部去查他域取角色）。
  Entity 全局共享，跨表查询允许在 repo 里直接用相关 Entity。
- **service 是业务编排层**：跨实体 / 跨域查询的组合发生在 service 层（先解析 A 域角色，
  再查 B 域数据）。
- **跨域复用带业务规则的操作**（唯一性校验、状态机、审计盖章、权限、软删策略）必须走
  该域 service；repo 层不重复实现他域规则。
- **例外——系统动作的日志/审计写入与过期数据清理**：这些是「接收已组装记录落库 / 按
  cutoff 物理删除」的无规则原语，中间件（`op_log` 写 `sys_operation_log`）、调度执行体
  （`scheduler` 写 `sys_job_log`）、认证（`auth` 写 `sys_login_log`）、定时清理任务
  （`task/*` 清过期日志）可直接调目标域 repo，不必为纯透传包 service。若该类写入未来
  新增业务规则（如审计要求），再改经 service。
- **判断口诀**：新增跨域数据需求时先问「调用目标携带目标域业务规则吗？」—— 是：走
  service；否（纯原语 / 只读组合）：service 层解析入参后调 repo。
- task 是新任务的注册边界：任务跨域只调用各域 repo 原语或 service（视上一条规则），
  不直接操作其他域 Entity 的业务逻辑。

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
- 现状：`created_by` / `updated_by` 已落地；名称拼装协议已实现（`utils/user_ref.rs`），
  本节为唯一事实来源。

## 测试规范

- 集成测试内联在各域 `repo.rs` / `service.rs`，直连 MySQL；纯单元测试在 `utils/`
- **事务回滚隔离（默认写法）**：业务层函数参数一律 `&impl ConnectionTrait`，
  测试夹具 `test_txn()` = 真库连接 + `begin()`，测试结束（含 panic 时
  `DatabaseTransaction` 的 Drop 自动 ROLLBACK）不留孤儿数据，**无需手写清理**：

  ```rust
  #[tokio::test]
  async fn xxx_yyy() {
      let db = test_txn().await;          // 替代旧 test_db() + 尾部 cleanup
      /* seed / 断言，无需任何清理语句 */
  }
  ```
- **例外**：内部自带事务的 repo 函数（`*_with_links` 系与 `soft_delete_user /
  soft_delete_role / soft_delete_menu / soft_delete_api`）保持 `&DatabaseConnection`
  —— sea-orm 1.1.20 真库无 savepoint，事务内嵌套 `begin()` 会被 MySQL 隐式提交，
  破坏回滚隔离。它们的测试仍用 `test_db()` 真连接 + 手写清理（函数注释有标注）；
  调用它们的 service 上游函数（`create/update/delete_user|role|api`、`delete_menu`）
  同为例外。升级 sea-orm 或将事务边界上移 service 后可取消例外。
- 测试数据唯一命名（`<prefix>_<pid>_<seq>`）仍保留：防止撞真实库的唯一键
  （如 `sys_user.username`、`sys_dictionary.type`）
- 命名格式：`<行为>_<场景>`，如 `find_page_filters_by_keyword_and_status_excludes_deleted`
- 运行单个模块：`cargo test <模块名>`（如 `cargo test role`）
- 业务层新增函数禁止新增 `db.begin()`：需要多表原子性时，先把事务边界上移到
  service 层并同步评估测试影响

## 提交与 PR 规范

- Conventional Commits + 中文描述，如
  `feat(rbac): 完善软删除过滤与用户创建校验`、`chore(db): 补充 RBAC 表和字段注释`
- 类型：`feat` / `fix` / `refactor` / `chore` / `docs` / `test`
- PR：说明改动目的、附验证证据（`cargo test` 结果）、契约变更需同步说明响应体与端点

## 协作约定

- W3 起固定分工：AI 编写失败测试并做最终 review，用户手动实现业务代码
- 新功能先建计划文档（`docs/superpowers/plans/`），按测试红 → 实现 → 验证推进
- 测试与迁移需要连接本地 MySQL，在非沙箱环境执行
