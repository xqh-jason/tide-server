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
