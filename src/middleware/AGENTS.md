## OVERVIEW

`src/middleware/` — 请求链路上的 5 个横切中间件 + `InjectState`（手写等价 `affix_state::inject`）。

**建档理由**：得分 9（`AuthRequired` 31 处 / 14 文件、`AuthUser` 16 文件导入 / 70 处、`ApiPermission` 22 处 / 8 文件；独立领域：全部请求都要穿这一层）。

> 计数口径（本机实测，2026-09-19）：
> ```sh
> grep -ro 'AuthRequired' src --include='*.rs' | wc -l   # 31
> grep -rl 'AuthRequired' src --include='*.rs' | wc -l   # 14
> grep -ro 'AuthUser'     src --include='*.rs' | wc -l   # 70
> grep -rl 'AuthUser'     src --include='*.rs' | wc -l   # 16
> grep -ro 'ApiPermission' src --include='*.rs' | wc -l  # 22
> grep -rl 'ApiPermission' src --include='*.rs' | wc -l  # 8
> ```

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| `AppState` 注入 Depot | `mod.rs::InjectState` | `depot.insert_typed(self.0.clone())`，handler 侧 `AppState::from_depot` |
| JWT 认证、会话有效性 | `auth.rs::AuthRequired` | 产出 `AuthUser`；`Authorization: Bearer <token>` |
| 接口级授权（`path + method`） | `api_permission.rs::ApiPermission` | 消费 `sys_api` / `sys_role_api` |
| 操作日志落库 | `op_log.rs::OperationLog` | 直写 `operation_log::repo` |
| 跨源白名单 | `cors.rs::Cors` | 构造时取 `config.cors` 副本，不依赖 Depot；挂载位置见 `infra/AGENTS.md` |
| 请求级超时兜底 | `request_timeout.rs::RequestTimeout` | 预算 30s（`DEFAULT_REQUEST_TIMEOUT_SECS`）+ `is_exempt` 路径豁免 |
| 中间件挂载顺序 | `infra/router.rs`（不在本目录） | 档位与顺序由挂载行的 `MountGuard` 决定；挂载行来自 `modules::DOMAINS` 登记表；公开挂位下需登录态的子路由（`auth/logout`、`site-config/update`）在各自域 `mod.rs` 自挂 |

## CONVENTIONS

- 三件套顺序固定 `AuthRequired → OperationLog → ApiPermission`：授权失败的写操作仍会留操作日志，这是有意的排列。
- `AuthRequired` 是业务响应**唯一**的 HTTP 状态码例外（401）；其余失败一律 HTTP 200 + `code: 0`。CORS 预检的 204 / 403 不进业务路由，属 CORS 协议自身语义，不算破坏契约。
- 认证热路径成本 = 一次合并会话查询（`refresh_token::repo::find_usable_with_user_by_id`，靠 `sys_refresh_token` 的唯一非空 `Relation` 做 `find_also_related` JOIN）+ 一次 60s 节流的 `touch_last_active_at` UPDATE。
- `ApiPermission` 双语义：**未登记的 `path + method` 一律放行**（fail-open，`sys_api` 空表与新增接口零影响，按域逐步登记接管）；**授权层自身故障宁可拒绝也不放行**（fail-closed）。授权失败不用 403，走契约体。
- `op_log` 只记 POST：非 POST 与只读后缀（`READ_ONLY_SUFFIXES`：`/list`、`/get`、`/info`、`/download` 等）一律不落日志——审计的是「谁改了什么」，只读查询是噪音。
- `op_log` 敏感键脱敏：`SENSITIVE_KEYS = [password, old_password, new_password, token, authorization, secret]` → `"***"`；请求体截断 `MAX_BODY_BYTES = 4096`；`redact` 是递归的（JSON 深度不设限）。
- `request_timeout` 用 `tokio::select!` 对撞预算，超时分支渲染契约体（不产生 4xx / 5xx）；`is_exempt` 只豁免两个流式端点 `/file/upload`、`/file/download`（慢网下大对象传输天然超预算）。
- 超管短路在 `permission::service::has_api_permission`（`ApiPermission` 的判定委托方）：实时加载的角色含 `super` 即放行、不查 `sys_role_api`，且不信任 JWT 里的角色快照。
- 手写 Salvo 附加件而非开 feature：`InjectState`、`Cors`、`RequestTimeout` 都是等价实现，原因是 `affix-state` 依赖 salvo_extra 0.95.2、rsproxy 镜像暂无该版本。

## ANTI-PATTERNS

- 不开 salvo 的 `affix-state` feature，不改回 `affix_state::inject`。
- 不把 CORS 挂到 `Router` 层（挂载位置与理由见 `infra/AGENTS.md`）。
- 不给流式端点加超时、也不给普通端点开豁免（豁免名单按路径精确判定，只此两条）。
- 操作日志不落敏感原文、不落完整大 body。
- 不再维护 token 黑名单：`auth.rs` 注明「黑名单已退役」，会话失效走 `sys_refresh_token` 吊销 + 过期判定。
