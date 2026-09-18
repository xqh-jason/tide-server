## OVERVIEW

`src/middleware/` — 请求链路上的 5 个横切中间件 + `InjectState`（手写等价 `affix_state::inject`）。

**建档理由**：得分 10（`AuthRequired` 30 次 / 14 文件、`OperationLog` 19 次、`ApiPermission` 16 次；独立领域：全部请求都要穿这一层）。

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| `AppState` 注入 Depot | `mod.rs::InjectState` | `depot.insert_typed(self.0.clone())`，handler 侧 `AppState::from_depot` |
| JWT 认证、会话有效性 | `auth.rs::AuthRequired` | 产出 `AuthUser`；`Authorization: Bearer <token>` |
| 接口级授权（`path + method`） | `api_permission.rs::ApiPermission` | 消费 `sys_api` / `sys_role_api` |
| 操作日志落库 | `op_log.rs::OperationLog` | 直写 `operation_log::repo` |
| 跨源白名单 | `cors.rs::Cors` | 构造时取 `config.cors` 副本，不依赖 Depot；挂载位置见 `infra/AGENTS.md` |
| 请求级超时兜底 | `request_timeout.rs::RequestTimeout` | 预算 30s（`DEFAULT_REQUEST_TIMEOUT_SECS`）+ `is_exempt` 路径豁免 |
| 中间件挂载顺序 | `infra/router.rs`（不在本目录） | 档位与顺序由挂载行的 `MountGuard` 决定；行来源是 `modules::DOMAINS` 或 `all_domains` 合并结果 |

## CONVENTIONS

- 三件套顺序固定 `AuthRequired → OperationLog → ApiPermission`：授权失败的写操作仍会留操作日志，这是有意的排列。
- `AuthRequired` 是全仓**唯一**使用 HTTP 状态码的例外（401）；其余失败一律 HTTP 200 + `code: 0`。
- 认证热路径成本 = 一次合并会话查询（`refresh_token::repo::find_usable_with_user_by_id`，靠 `sys_refresh_token` 的唯一非空 `Relation` 做 `find_also_related` JOIN）+ 一次 60s 节流的 `touch_last_active_at` UPDATE。
- `ApiPermission` 双语义：**未登记的 `path + method` 一律放行**（fail-open，`sys_api` 空表与新增接口零影响，按域逐步登记接管）；**授权层自身故障宁可拒绝也不放行**（fail-closed）。授权失败不用 403，走契约体。
- `op_log` 敏感键脱敏：`SENSITIVE_KEYS = [password, old_password, new_password, token, authorization, secret]` → `"***"`；请求体截断 `MAX_BODY_BYTES = 4096`；`redact` 是递归的（JSON 深度不设限）。
- `request_timeout` 用 `tokio::select!` 对撞预算，超时分支渲染契约体（不产生 4xx / 5xx）；`is_exempt` 只豁免两个流式端点 `/file/upload`、`/file/download`（慢网下大对象传输天然超预算）。
- 超管短路：`ApiPermission` 对 `SUPER_ROLE_KEY` 直接放行，不再查 `sys_role_api`。
- 手写 Salvo 附加件而非开 feature：`InjectState`、`Cors`、`RequestTimeout` 都是等价实现，原因是 `affix-state` 依赖 salvo_extra 0.95.2、rsproxy 镜像暂无该版本。

## ANTI-PATTERNS

- 不开 salvo 的 `affix-state` feature，不改回 `affix_state::inject`。
- 不把 CORS 挂到 `Router` 层（挂载位置与理由见 `infra/AGENTS.md`）。
- 不给流式端点加超时、也不给普通端点开豁免（豁免名单按路径精确判定，只此两条）。
- 操作日志不落敏感原文、不落完整大 body。
- 不再维护 token 黑名单：`auth.rs` 注明「黑名单已退役」，会话失效走 `sys_refresh_token` 吊销 + 过期判定。
