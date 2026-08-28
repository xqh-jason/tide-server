# W3 权限码授权计划：按钮权限码 + 后端显式校验

> 目标：让 `/api/v1/user/create` 从“已登录即可访问”升级为“拥有 `system:user:create` 的有效用户才能访问”。
>
> 协作约定：AI 编写测试和最终 review；用户手动实现业务代码。

## 已确认方案

- 以 `sys_menu.permission` 作为第一版权限码主数据。
- `menu_type = 3` 表示按钮权限，`status = 1` 且 `deleted_at IS NULL` 才有效。
- 前端通过 `/access-codes` 拿到权限码后控制按钮显隐；这只是交互体验。
- 后端受保护接口必须独立校验同一权限码；这是安全边界。
- `role_key = super` 作为超级管理员短路规则，但必须基于数据库实时角色判断，不能只信任 JWT 旧快照。
- `sys_api` 与 `sys_role_api` 保留表结构，暂不作为第一版主授权数据源。

## 实施顺序

### 1. 固化项目约定

- [x] 在 `src/modules/mod.rs` 写入数据有效性约定。
- [x] 在 `src/modules/mod.rs` 写入权限码共享语义与 W3 第一版授权模式。

### 2. 权限常量

- [x] 新增权限码模块或常量定义，避免在业务代码中重复手写字符串。
- [x] 第一批至少包含 `system:user:create`。

### 3. AI 编写权限查询失败测试

- [x] `super` 用户具有全部操作权限。
- [x] 普通用户绑定有效按钮菜单后拥有对应权限码。
- [x] 禁用角色的权限被忽略。
- [x] 已删除角色的权限被忽略。
- [x] 禁用按钮菜单的权限被忽略。
- [x] 已删除按钮菜单的权限被忽略。
- [x] 关系表本身硬删除，不判断关系表软删除字段。

当前测试包括：有效绑定、禁用/删除角色、无效按钮、按钮类型过滤和 `super` 授权短路，
全部在 `src/modules/permission/repo.rs` 的集成测试中覆盖。

### 3a. 修复测试发现的表结构问题

- [x] 恢复 `sys_menu.parent_id` 的 `DEFAULT 0`。
  - 初始建表迁移定义了默认值，但字段注释迁移重定义列时丢失了它。
  - 当前开发库 `SHOW CREATE TABLE sys_menu` 已显示 `parent_id bigint unsigned NOT NULL`
    且无默认值。
  - 可通过新增小迁移恢复默认值；不要只依赖测试代码显式传 `0`。

### 4. 用户实现权限查询

- [x] 新建权限域或最小模块，提供按 `user_id` 查询当前有效权限码的能力。
- [x] 主查询链路使用 `user_id -> sys_user_role -> sys_role -> sys_role_menu -> sys_menu`。
- [x] 过滤规则：
  - 用户：`deleted_at IS NULL AND status = 1`（由认证中间件 `ensure_user_active` 统一过滤，
    权限查询层不再重复查用户表）
  - 角色：`deleted_at IS NULL AND status = 1`
  - 按钮：`deleted_at IS NULL AND status = 1 AND menu_type = 3 AND permission <> ''`
  - 关系表：只表示绑定关系，不做自身软删除过滤。

> 实现说明（按用户反馈调整）：不提供 `find_role_keys_by_user_id`，`has_permission` 直接复用
> `find_roles_by_user_id` 判断 `super`；用户有效性收敛到认证中间件，JWT 验证通过后查库确认
> 用户存在、未删除、启用，否则统一 401。

### 5. AI 编写创建用户接口授权测试

- [x] 无登录请求被认证层拒绝（`AuthRequired` 中间件保证，`/api/v1/user/*` 整体挂载）。
- [x] 登录但缺少 `system:user:create` 时拒绝，且不创建目标用户。
- [x] 有 `system:user:create` 的普通用户可以创建用户。
- [x] 当前数据库中存在有效 `super` 角色的用户可以创建用户。
- [x] JWT 中保留了旧角色、但数据库中角色已被删除或禁用时拒绝。
- [x] 发起人已被软删除时，即使 token 仍有效也拒绝（中间件 `ensure_user_active` 测试覆盖）。

### 6. 用户实现接口校验

- [x] 在 `create_user` 执行业务前调用权限校验（`create_user(db, actor_id, req)`，
  handler 从 `AuthUser` 取发起人 ID）。
- [x] 校验通过后再执行现有用户名唯一性检查、角色存在性检查和事务写入。
- [x] 保持无权限时不产生任何用户或用户角色关联写入。

### 7. 完善 `/access-codes`

- [x] 复用按用户查询有效权限码的仓储能力（`get_access_codes(db, user_id)` 实时查库，
  不信任 JWT 旧角色快照）。
- [x] `super` 返回明确的超管标识（`['super']`）。
- [x] 普通用户返回其实际可用的按钮权限码列表。
- [x] 保证返回结果去重、排序方式稳定，便于前端消费和测试断言。

### 8. 收尾验证

- [x] `cargo fmt --check`
- [x] `cargo test`（45 passed / 0 failed）
- [x] Review 数据库过滤条件是否遗漏主表软删除状态。
- [x] Review 是否存在绕过权限校验直接写入用户的路径。

> 收尾结论：写用户仅 `api.rs -> service::create_user -> repo::create_user_with_roles` 一条
> 业务链路，入口已带权限校验，无绕过路径；过滤条件（用户/角色/按钮主表软删除与启用状态）
> 完整，关系表不判软删除符合约定。
