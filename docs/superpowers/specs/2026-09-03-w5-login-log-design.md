# W5 登录日志模块设计规格

> 日期：2026-09-03
> 状态：待用户 review
> 模块位置：W5 通用模块第 2 项（操作日志 → 登录日志 → 数据字典 → 文件上传 → 图形验证码 → 系统配置）

## 1. 背景与目标

W5-1 操作日志已落地（中间件自动记录已登录业务请求）。登录发生在认证中间件
之前，操作日志覆盖不到，因此单独提供登录日志：登录成功与失败自动落库，
失败原因内部分级；管理端只读分页 / 详情 + 单删 / 批量删（软删）。

语义对齐 gin-vue-admin 的 `sys_login_logs`，表名按本项目单数规范为
`sys_login_log`；契约遵守统一 `POST + JSON`、`code = 1 / 0`。

## 2. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 写入位置 | 登录 service 内写（成功与失败都记录），IP / UA 由 handler 传入 |
| 2 | 管理端能力 | 只读 + 软删：list / get / delete / delete-batch，不提供清空 |
| 3 | 失败 msg | 内部分级（用户不存在 / 密码错误 / 用户已被禁用 / token 签发失败） |
| 4 | 对外文案 | 保持防枚举统一提示；禁用用户泄漏提示顺带修复 |

## 3. 表结构

表名：`sys_login_log`。

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | 主键 |
| user_id | BIGINT UNSIGNED | NOT NULL DEFAULT 0 | 登录成功才有值；失败为 0 |
| username | VARCHAR(64) | NOT NULL | 本次尝试的用户名（账号不存在也保留） |
| ip | VARCHAR(64) | NOT NULL DEFAULT '' | 来源 IP |
| agent | VARCHAR(255) | NOT NULL DEFAULT '' | User-Agent 原文（W5 不做浏览器解析） |
| status | TINYINT | NOT NULL | 1 成功 / 0 失败 |
| msg | VARCHAR(255) | NOT NULL DEFAULT '' | 成功“登录成功”；失败为内部原因 |
| created_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP | 登录时间 |
| updated_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE | |
| deleted_at | DATETIME | NULL | 软删时间 |

索引：

- `idx_sys_login_log_created_at`（created_at DESC）；
- `idx_sys_login_log_username`（username）。

无唯一字段，正好作为 codegen「无唯一字段域」模板修复后的首个验证域。
登录路径不单独存储：本项目固定单一登录端点。

## 4. 写入位置与数据流

```text
POST /api/v1/auth/login（公开）
  → handler 从 Request 取 IP / UA
  → 构造 LoginMeta { ip, agent } 传给 service
  → service::login 内部按分支记录 sys_login_log
  → 返回业务结果（日志失败只 tracing 降级，不影响登录）
```

msg 分级：

| 场景 | status | msg | 对外提示 |
|---|---|---|---|
| 登录成功 | 1 | 登录成功 | token |
| 用户不存在 | 0 | 用户不存在 | 用户名或密码错误 |
| 密码错误 | 0 | 密码错误 | 用户名或密码错误 |
| 用户已被禁用 | 0 | 用户已被禁用 | 用户名或密码错误 |
| token 签发失败 | 0 | token 签发失败 | 系统错误（code=0） |

防枚举修复：现代码在用户 status != 1 时提前返回「用户已被禁用」，对外泄漏
账号状态且存在冗余的第二个 status 检查；本模块将其合并为「先查用户 → 校验
密码 → 校验状态」，对外统一「用户名或密码错误」，日志 msg 仍写
「用户已被禁用」。

入库前截断：username / ip / msg 按列长安全截断（字符边界），agent 截断到
255 字符，避免超长触发 DB 报错。

## 5. 接口契约

路由前缀：`/api/v1/login-log`（AuthRequired 保护）。

| 端点 | 请求体 | 说明 |
|---|---|---|
| POST /list | `{ page, username?, ip?, status? }` | username / ip 模糊，status 精确；created_at 倒序 |
| POST /get | `{ id }` | 单条详情 |
| POST /delete | `{ id }` | 软删单条；不存在返回 Biz |
| POST /delete-batch | `{ ids: [u64] }` | 批量软删，跳过不存在；空数组返回 0 |

列表与详情使用同一响应结构（无 body / resp 大字段，不需要拆分 DTO）：

```json
{
  "code": 1,
  "data": {
    "total": 1,
    "total_pages": 1,
    "items": [
      {
        "id": 1,
        "user_id": 1,
        "username": "admin",
        "ip": "127.0.0.1",
        "agent": "curl/8.0",
        "status": 1,
        "msg": "登录成功",
        "created_at": "2026-09-03 10:00:00"
      }
    ]
  },
  "message": "ok"
}
```

不存在记录时单删 / 详情返回 `登录日志不存在：{id}`。

## 6. 菜单与权限码种子

在 MENU_SEEDS 追加（System 下 sort 6，操作日志之后）：

| 项 | 值 |
|---|---|
| name | SystemLoginLog |
| title | 登录日志 |
| path | /system/login-log |
| component | #/views/system/login-log/index.vue |
| icon | lucide:history |
| menu_type | 2 |
| parent | System |
| sort | 6 |

按钮（menu_type 3，parent = SystemLoginLog）：

- `system:login-log:delete`

查询入口由菜单权限控制，不单独配置 list 权限码。

## 7. 错误处理与降级

- 日志落库失败：`tracing::error!` 后按原登录结果返回，不阻断登录；
- DB 技术故障分支（用户查询失败）：尝试记录失败日志（可能同样失败），
  随后仍返回原技术错误；
- 管理端删除不存在记录：单删返回 Biz；批量删除静默跳过。

## 8. 测试策略

### 8.1 repo 集成测试

- 分页按 username / ip 模糊、status 精确过滤，排除软删，created_at 倒序；
- 单删不存在返回 false；批量软删只处理存在且未删除的行；
- 测试数据唯一命名并物理清理。

### 8.2 service 测试（扩展 auth service 现有登录测试）

- 登录成功 → 表内新增 status=1、user_id、msg=登录成功 的行；
- 密码错误 / 用户不存在 / 用户禁用 → status=0 且 msg 分级正确；
- 三种失败对外返回的 AppError 文案统一为「用户名或密码错误」；
- 日志落库失败不影响登录：review 层面确认（mock DB 成本高，不做自动测试）。

### 8.3 种子测试

- ensure_seed 后 SystemLoginLog 菜单与 system:login-log:delete 按钮存在。

## 9. 范围外

- 登录日志清空 / 自动清理（W6 定时任务）；
- browser / OS / 归属地解析（W5 只存原始 UA）；
- logout 记录（已由操作日志覆盖）；
- 前端登录日志页面（前端仓库另行实现）。

## 10. 参考实现位置

- 登录业务：`src/modules/auth/{api,dto,service}.rs`
- 迁移注册：`migrations/src/lib.rs`
- 路由挂载：`src/infra/router.rs`
- 种子数据：`src/infra/seed.rs`
- 无唯一字段域模板：`codegen/src/templates.rs`（W5-1 已修复，本域为验证域）
