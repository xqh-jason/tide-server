# W5 操作日志模块设计规格

> 日期：2026-09-03
> 状态：待用户 review
> 模块位置：W5 通用模块第 1 项（按学习计划顺序：操作日志 → 登录日志 → 数据字典 → 文件上传 → 图形验证码 → 系统配置）

## 1. 背景与目标

当前项目已完成 W4 代码生成器，并以 sys_dict 域验证；W5 开始落地通用模块。
本规格定义「操作日志」模块：所有已登录业务请求由中间件自动记录，
管理端提供只读分页 / 详情 / 删除能力，语义对齐 gin-vue-admin 的
`sys_operation_records`，同时遵守本项目既有约定：

- 统一契约：所有接口 `POST + JSON body`，成功 `code = 1`，失败 `code = 0`；
- 表名单数命名、软删除主表默认过滤 `deleted_at IS NULL`；
- 主表软删、接口命名与路由顺序遵循垂直切片四件套约定；
- 代码骨架由生成器产出，业务实现由用户手动完成，测试由 AI 编写并做最终 review。

## 2. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 落地方式 | 中间件自动落库 + 管理端分页查询 |
| 2 | 记录范围 | 全部已登录业务请求（login / health 等公开接口不经过） |
| 3 | body / resp | 都存；敏感键脱敏、各自截断到 4 KB |
| 4 | 管理端能力 | 只读 + 软删：list / get / delete / delete-batch |
| 5 | 菜单与权限 | 本轮补齐种子：系统管理 → 操作日志菜单 + delete 权限码 |

## 3. 表结构

表名：`sys_operation_log`（本项目单数命名，与 sys_dict / sys_user 一致；
gin-vue-admin 对应表为 `sys_operation_records`，字段语义对齐）。

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | 主键 |
| user_id | BIGINT UNSIGNED | NOT NULL | 操作人 ID；不加外键，用户删除后日志保留 |
| ip | VARCHAR(64) | NOT NULL DEFAULT '' | 来源 IP |
| method | VARCHAR(16) | NOT NULL DEFAULT '' | 请求方法 |
| path | VARCHAR(255) | NOT NULL | 请求路径 |
| status | INT | NOT NULL DEFAULT 0 | HTTP 状态码（对齐 GVA 语义） |
| latency | BIGINT | NOT NULL DEFAULT 0 | 请求耗时，单位毫秒 |
| agent | VARCHAR(255) | NOT NULL DEFAULT '' | User-Agent |
| body | TEXT | NOT NULL | 请求体（脱敏 + 截断后） |
| resp | TEXT | NOT NULL | 响应体（脱敏 + 截断后） |
| error_message | VARCHAR(500) | NOT NULL DEFAULT '' | 业务失败提示，无则空串 |
| created_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP | |
| updated_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE | |
| deleted_at | DATETIME | NULL | 软删时间 |

索引：

- `idx_sys_operation_log_created_at`（created_at DESC）：列表默认倒序；
- `idx_sys_operation_log_user_id`（user_id）：按操作人筛选。

实体 `sys_operation_log` 的 `status` 与列表过滤中的 `status` 均表示 HTTP 状态码，
不是业务启用状态，注释中必须写清楚。

## 4. 中间件与数据流

新增中间件 `OperationLog`（文件：`src/middleware/op_log.rs`），挂载在
AuthRequired 之后的所有受保护业务路由上（login / health / 401 响应不记录）。

数据流：

```text
请求进入受保护路由
  → AuthRequired 注入 AuthUser（未登录直接 401，不产生日志）
  → OperationLog 记录开始时间，预读请求体并放回 req
  → ctrl.call_next 执行业务 handler
  → 业务结束：取响应体并放回 res
  → 组装日志：user_id / ip / method / path / status / latency / agent /
    body / resp / error_message
  → 脱敏 + 截断 → await 落库（失败只记 tracing 错误，不影响业务响应）
```

实现要点：

- 请求体预读：进入时 `req.take_body()` 读原始 JSON，截断备份后
  `req.replace_body(...)` 放回，保证 handler 的 JsonBody 可正常解析；
- 响应体捕获：`ctrl.call_next` 后 `res.take_body()` 读取并 `replace_body` 放回，
  保证客户端仍能收到完整响应；
- user_id 来自 Depot 中 AuthRequired 注入的 `AuthUser`；
- 本模块自己的管理接口同样经过中间件并产生日志；
- 落库放在请求路径内 await：便于集成测试断言且保证审计完整；
  落库失败不得让业务返回失败，只 `tracing::error!` 降级。

## 5. 接口契约

路由前缀：`/api/v1/operation-log`（AuthRequired 保护）。

| 端点 | 请求体 | 说明 |
|---|---|---|
| POST /list | `{ page, keyword?, user_id?, status? }` | 分页；keyword 对 path 模糊；默认 created_at 倒序；列表项不含 body / resp |
| POST /get | `{ id }` | 详情；含脱敏截断后的 body / resp |
| POST /delete | `{ id }` | 软删单条 |
| POST /delete-batch | `{ ids: [u64] }` | 软删批量；空数组表示无操作，不执行删除 |

模块组织：`src/modules/operation_log/`（api / service / repo / dto + mod.rs），
实体与表均为 `sys_operation_log`；URL 路径与菜单 component 使用连字符
`operation-log`。

列表响应体示例：

```json
{
  "code": 1,
  "data": {
    "total": 2,
    "total_pages": 1,
    "items": [
      {
        "id": 1,
        "user_id": 1,
        "ip": "127.0.0.1",
        "method": "POST",
        "path": "/api/v1/user/list",
        "status": 200,
        "latency": 12,
        "agent": "curl/8.0",
        "error_message": "",
        "created_at": "2026-09-03T10:00:00"
      }
    ]
  },
  "message": "ok"
}
```

详情响应在列表字段基础上追加 `body` / `resp` 两个字段。

不存在记录时，get / delete 返回业务错误（code=0）：

- 查询/删除不存在：`操作日志不存在：{id}`；
- delete-batch 不存在的 id：静默跳过（符合“按 id 批量清理”语义）。

## 6. 脱敏与截断规则

- 敏感键集合：`password`、`old_password`、`new_password`、`token`、
  `authorization`、`secret`；对 body / resp 做 JSON 递归遍历，命中键的
  值一律替换为 `***`；
- body / resp 各自截断到 4096 字节（UTF-8 安全边界截断，超长尾部追加
  `...(截断)`）；
- agent 超过 255 字符截断；ip 超过 64 字符截断。

## 7. 菜单与权限码种子

在 `src/infra/seed.rs` 的 MENU_SEEDS 中追加（排序 System 下第 5 位）：

| 项 | 值 |
|---|---|
| name | SystemOperationLog |
| title | 操作日志 |
| path | /system/operation-log |
| component | #/views/system/operation-log/index.vue |
| icon | lucide:scroll-text |
| menu_type | 2（页面） |
| parent | System |
| sort | 5 |

按钮权限（menu_type 3，parent = SystemOperationLog）：

- `system:operation-log:delete`（title：操作日志删除，sort 1）

查询入口由菜单本身控制，不单独配置 list 权限码。现有 super 角色全量绑定
逻辑自动覆盖新增菜单与按钮；ensure_seed 幂等要求保持不变。

## 8. 错误处理与降级

- 业务失败（handler 返回 code=0）也要落库：status 为 HTTP 状态码，
  error_message 记录业务 message，resp 存脱敏后的失败契约体；
- 401 / 未登录场景由 AuthRequired 提前拦截，不产生日志；
- 请求体 / 响应体捕获失败时：对应字段存空串，不得中断请求；
- 日志落库失败：`tracing::error!` 后继续返回业务结果。

## 9. 测试策略

### 9.1 repo 集成测试（直连 MySQL，测后清理）

- 分页按 keyword（path 模糊）/ user_id / status 过滤并排除软删；
- 默认按 created_at 倒序；
- delete-batch 批量软删后查询不可见；
- 删除不存在记录返回 false，由 service 层转 Biz 错误。

### 9.2 service 测试

- get / delete 不存在记录返回 `AppError::Biz`；
- delete-batch 空数组不执行删除。

### 9.3 中间件集成测试

- 发一个受保护业务请求后，表内出现对应日志（user_id / path / body 等正确）；
- body 含 password 时入库内容已替换为 `***`；
- handler 返回业务失败时日志仍落库，error_message 与 resp 记录失败内容；
- 日志落库失败不影响业务响应（可通过注入失败 DB 或 mock 验证，若成本过高
  则降级为代码 review + 单测覆盖脱敏函数）。

### 9.4 种子测试

- ensure_seed 后菜单 SystemOperationLog 与按钮权限码存在；
- 总数断言跟随 MENU_SEEDS.len()，不硬编码。

## 10. 范围外

- 登录日志模块（W5 第 2 项，单独规格）；
- 日志自动清理 / 归档（W6 定时任务阶段）；
- 前端操作日志页面（前端仓库另行实现）；
- 数据字典 gin-vue-admin 对齐（W5 第 3 项，单独规格）。

## 11. 参考实现位置

- 迁移注册：`migrations/src/lib.rs`
- 路由挂载：`src/infra/router.rs`
- 种子数据：`src/infra/seed.rs`
- 认证中间件范式：`src/middleware/auth.rs`
- 提取器与错误契约：`src/utils/request.rs`、`src/utils/response.rs`
- 分页通用件：`src/utils/page.rs`
- codegen 域定义：`codegen/defs/dict.json`（操作日志定义放同目录）
