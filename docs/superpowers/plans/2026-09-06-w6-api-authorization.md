# W6 接口级权限授权层（ApiPermission 中间件）实现计划

> **面向 AI 代理的工作者：** 本计划遵循协作约定——**Rust 实现全部由用户手写**，
> AI 提供任务拆分、签名骨架与失败测试；用户实现后由 AI 做 review。
> 步骤中用 **[AI]** / **[用户]** 标注执行者，复选框跟踪进度。
> 背景：`sys_api` / `sys_role_api` 表已就绪但未参与主授权链路
> （`modules/mod.rs` 声明"后续再引入"；`user delete` 注释预留"接口级权限码由
> 后续 API 授权层统一施加"）。本计划补上后端双保险。

---

## 设计决策

1. **授权模型**：请求 `path + method` 精确匹配 `sys_api`（`deleted_at IS NULL AND status=1`），
   再校验用户实时有效角色与该 API 的 `sys_role_api` 授权交集；超管短路放行。
   判定内核复用 `user_repo::find_roles_by_user_id`（不信任 JWT 角色快照），
   范式对齐 `permission/service.rs::has_permission`。
2. **fail-open（未登记放行）**：`sys_api` 现为空表，fail-closed 会上线即全站拒绝；
   未登记接口放行、已登记必须授权，后续按域逐步登记接管。
3. **决策逻辑放 service**：`has_api_permission`（收集 → 判定），中间件只做取状态、
   调判定、渲染失败响应的薄壳。
4. **响应契约**：授权失败 = HTTP 200 + `code=0` + `message="无该接口访问权限"`
   （项目契约仅 401 例外），渲染参考 `middleware/auth.rs::unauthorized` + `ctrl.skip_rest()`。
5. **挂载顺序**：`AuthRequired → OperationLog → ApiPermission`——授权失败的写操作
   仍留操作日志（审计最需要记录的就是被拒请求）。10 个域组各加一个 hoop；
   `auth/logout`、`site-config/update`、公开路由（health/login/captcha/site-config/get）不挂。
6. **不加迁移 / 不加缓存 / 不动 seed**：v1 表保持空（fail-open 下零影响），
   接口登记由用户经 sys-api 管理端点逐步录入；service 层现有权限码校验全部保留
   （按钮码 + 接口授权双通道并存）；缓存优化留后续（v1 每请求 2~3 条索引小查询）。
7. **唯一索引约束**：`uk_api_path_method` 对 (path, method) 物理唯一（含软删行占位），
   测试造数每行用不同 path。

## 文件结构

创建：
- `src/middleware/api_permission.rs`（ApiPermission 中间件骨架）
- 本计划文档

修改：
- `src/middleware/mod.rs`（`pub mod api_permission;`）
- `src/modules/permission/repo.rs`（2 个新查询函数 + 3 个失败测试）
- `src/modules/permission/service.rs`（`has_api_permission` + 5 个失败测试）
- `src/infra/router.rs`（10 处域组追加 `.hoop(ApiPermission)`，实现完成后挂载）

---

## 任务 1：失败测试 + 骨架（[AI]）

- [x] **[AI]** repo 失败测试 3 个（`test_txn` 事务回滚风格）：
  - `find_active_api_by_path_method_returns_none_for_unregistered_path`
  - `find_active_api_by_path_method_excludes_deleted_and_disabled`（含 method 不匹配）
  - `exists_role_api_matches_intersection_only`（含空 role_ids 短路）
- [x] **[AI]** service 失败测试 5 个：
  - `has_api_permission_allows_unregistered_endpoint`
  - `has_api_permission_short_circuits_for_super`
  - `has_api_permission_allows_authorized_role`
  - `has_api_permission_rejects_unauthorized_role`（含无角色用户）
  - `has_api_permission_ignores_deleted_or_disabled_api`
- [x] **[AI]** 骨架：repo 两函数 + service 一函数签名（`todo!()` + 步骤提示）、
  `middleware/api_permission.rs` 四步流程骨架、`mod.rs` 注册
- [x] **[AI]** 确认红：`cargo test permission` 新增 8 个测试全部失败

## 任务 2：repo 层实现（[用户]）

- [x] **[用户]** `find_active_api_by_path_method`：path + method 精确匹配，
  过滤 `deleted_at IS NULL` 且 `status = 1`
- [x] **[用户]** `exists_role_api`：空 `role_ids` 直接返回 false（防空 IN），
  否则查 `sys_role_api` 中 `api_id` + `role_id IN (...)` 是否存在
- [x] **[AI]** 复跑 `cargo test permission` 验证 repo 测试绿

## 任务 3：service 层实现（[用户]）

- [x] **[用户]** `has_api_permission`：未登记放行 → 超管短路 → `sys_role_api`
  交集判定；技术错误 `?` 传播（`#[from]` 归为 Internal）
  （review 修正一轮：未登记分支由 `Err(Biz)` 改为 `Ok(true)` fail-open）
- [x] **[AI]** 复跑 `cargo test permission` 验证 service 测试绿（18/18）

## 任务 4：中间件实现（[用户]，本轮由用户委托 [AI] 代写）

- [x] **[AI]** `middleware/api_permission.rs`：取 `AppState` 与 `AuthUser`
  （缺失记 error 日志并拒绝）→ call_next 前拷贝 path/method → 调 service 判定
  → false 时 HTTP 200 + `ApiResponse::<()>::fail("无该接口访问权限")` + `skip_rest()`；
  技术错误记日志后同样拒绝（fail-closed）
- [x] **[AI]** review：薄壳边界（无业务逻辑）、401/403 语义区分、path 拷贝时机

## 任务 5：路由挂载 + 全量验证（[AI]）

- [x] **[AI]** `src/infra/router.rs` 10 个域组 `.hoop(OperationLog)` 后追加
  `.hoop(ApiPermission)`（user / menu / dictionary / dictionary-detail / role /
  sys-api / operation-log / login-log / file / config）
- [x] **[AI]** 全量验证：`cargo fmt --check`、`cargo check`、`cargo test`
  （199/199 全绿；既有 25 处 unused import 警告为 76ae558 事务重构遗留，另案清理）
- [x] **[AI]** 最终 review + 冒烟建议：经 sys-api 端点登记 `POST /api/v1/user/delete`、
  授权给某角色 → 未授权用户调该接口得 `code=0`、授权角色与超管放行、
  被拒请求在操作日志可见
- [ ] Commit：`feat(permission): 接口级授权层，按 sys_api/sys_role_api 校验角色（W6）`

---

## 骨架签名

```rust
// ── src/modules/permission/repo.rs ──
pub async fn find_active_api_by_path_method(
    db: &impl ConnectionTrait, path: &str, method: &str,
) -> anyhow::Result<Option<sys_api::Model>>;

pub async fn exists_role_api(
    db: &impl ConnectionTrait, api_id: u64, role_ids: &[u64],
) -> anyhow::Result<bool>;

// ── src/modules/permission/service.rs ──
pub async fn has_api_permission(
    db: &impl ConnectionTrait, user_id: u64, path: &str, method: &str,
) -> Result<bool, AppError>;

// ── src/middleware/api_permission.rs ──
pub struct ApiPermission;   // impl Handler：四步流程骨架见文件内注释
```

## 验收标准

- 新增 8 个测试全绿，主项目全量测试通过，`cargo fmt --check` 通过
- 未登记接口行为零变化（fail-open 回归）
- 登记 API 后：授权角色可访问、未授权角色收到 `code=0`、超管放行、被拒请求留操作日志
