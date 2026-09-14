# 生产环境 JWT Secret 加固设计（含会话管理与发版踢人分析存档）

- 日期：2026-09-14
- 状态：已实施（2026-09-14，见文末实施记录）
- 关联：`2026-09-13-auth-refresh-session-design.md`（refresh token 会话机制，commit `0467293`）

## 一、问题与结论（分析存档）

问题：后端项目重新发版，所有用户 token 都失效被踢出去了吗？这样是否合理？

结论：

1. **「全员被踢」只发生在 2026-09-13 落地 refresh token 会话机制的那次发版**。旧版 JWT
   载荷（W2 黑名单方案）缺 `refresh_token_id` 字段，新方案将其设为必填，serde 反序列化
   必然失败 → 旧 token 全员 401。这是设计预期的迁移路径：`src/utils/jwt.rs:13` 注释
   明示「发版后全员重登（预期迁移路径）」，并有 `legacy_token_without_refresh_token_id_fails`
   与 `auth_required_rejects_token_without_refresh_token_id` 两个测试固化。属一次性结构
   迁移成本，非事故。
2. **之后的常规发版（JWT secret 与数据库不变）不会踢人**：
   - JWT secret 来自 config.toml / `TIDE_JWT__SECRET` 环境变量，不随进程生命周期变化；
   - 会话状态在 MySQL `sys_refresh_token` 表（吊销/过期判定每请求一条合并 SQL），重启不丢；
   - 内存 cache（DashMap）只承担 `last_active_at` 60s 节流键与验证码，丢失无害；
   - access token（默认 2h）过期后，前端凭 HttpOnly Cookie 中的 refresh token（默认 7 天）
     静默调 `/auth/refresh` 续期，用户无感。
3. 会再踢人的三种情况：换 secret；清库 / 重建库；再次修改 `Claims` 结构（每改一次载荷
   需有意识地接受一次全员重登，或写兼容双轨）。
4. **方案整体合理**：DB 会话 + 无状态 JWT + 静默刷新是主流企业做法；即时吊销（登出 /
   强制下线）是纯 JWT 做不到的能力，此处用每请求一条 SQL 换到，管理后台量级完全可接受；
   refresh token 只存哈希、HttpOnly Cookie、明文只出现一次，安全姿势正确；一次性踢人
   相比长期双轨兼容代码更划算。

已知取舍（维持现状）：

- refresh token 不轮换（同一会话固定 7 天有效期）；严格做法是 rotation + 重放检测。
- `/auth/refresh` 成功响应体为裸 token 字符串（偏离统一 `{code, data, message}` 契约，
  已注释说明是对 vben `authenticateResponseInterceptor` 的适配）。
- 滚动发布新旧实例并存窗口内，旧 token 打到新实例会 401 → 前端静默刷新恢复；单机
  compose 部署无此窗口。

## 二、发现的唯一实质风险：生产 secret 兜底为公开开发密钥

- `docker-compose.yml:47`：`TIDE_JWT__SECRET: ${TIDE_JWT_SECRET:-dev-secret-change-me}`
- `config.toml:28`：`secret = "dev-secret-change-me"`

后果：

1. 生产漏设 `TIDE_JWT_SECRET` → 服务用公开的开发密钥静默启动，任何人可自签合法 token
   （安全隐患）；
2. 某次部署设了、下次漏设 → secret 变化 → 一次「不明所以」的全员踢出，且无告警可查。

## 三、加固设计：production 禁用默认开发 secret（fail-fast）

### 目标

- `env == "production"` 且 `jwt.secret` 为已知开发默认值时，启动即失败并提示设置专用密钥。

### 非目标

- 不改认证中间件、不改 `Claims`、不引入 refresh token 轮换（维持上述已知取舍）。

### 实施步骤（协作约定：AI 编写失败测试并做最终 review，用户手动实现业务代码）

1. **失败测试先行（AI）**：`src/infra/config.rs` 测试模块新增两个用例（复用现有
   `ENV_LOCK` + `EnvGuard` 模式）：
   - `TIDE_ENV=production` + `TIDE_JWT__SECRET=dev-secret-change-me` → `Config::load()`
     必须报错（文案提示设置专用密钥）；
   - `TIDE_ENV=production` + 自定义 secret → 正常通过。
   验证：`cargo test config` 确认红。
2. **业务实现（用户）**：`Config::load()` 现有校验段（`src/infra/config.rs:166-177`）
   追加：production 且 secret 属于已知开发默认值集合 → `anyhow::bail!`。集合收成常量
   数组（如 `KNOWN_DEV_SECRETS: [&str; 1] = ["dev-secret-change-me"]`）便于将来追加。
3. **部署配套同步**：
   - `docker-compose.yml:47` 去掉兜底默认值（改为 `${TIDE_JWT_SECRET:?未设置 TIDE_JWT_SECRET}`
     或留空交由后端 fail-fast 拦截）；
   - `.env.example` 对应条目同步注释；
   - README 部署段落若提及 `TIDE_JWT_SECRET`，补一句「production 下默认开发密钥会被
     拒绝启动」。
4. **验证（AI review + 跑绿）**：`cargo test config` 转绿；`cargo fmt --check`、
   `cargo check` 通过；`TIDE_ENV=production` 本地起服务确认 fail-fast 行为。

### 行为对照表

| 场景 | 现状 | 加固后 |
| --- | --- | --- |
| development + 默认 secret | 正常启动 | 正常启动（不变） |
| production + 默认 secret | 静默启动（隐患） | 启动即失败，提示设置专用密钥 |
| production + 自定义 secret | 正常启动 | 正常启动（不变） |
| compose 漏设 `TIDE_JWT_SECRET` | 回退 `dev-secret-change-me` | compose 层直接报错（`:?` 必设语法） |

## 四、实施记录（2026-09-14）

落地与计划的差异及验证结果：

1. **校验抽为纯函数 `ensure_production_secret(env, secret)`**（`src/infra/config.rs`），
   `Config::load()` 末尾一行接线。原因：原计划用 `TIDE_ENV` 环境变量篡改式测试，
   首次全量 `cargo test config` 即触发并行用例失败——`std::env` 是进程全局资源，
   fail-fast 生效后，测试窗口内其他用例的 `Config::load()`（连库夹具）会意外被拒。
   纯函数直测零污染，测试改为 3 个用例（production + 默认密钥拒绝 / production +
   自定义密钥放行 / development + 默认密钥放行）。
2. compose 侧选定 `:?` 必设方案（未设或为空即报错），不依赖后端兜底拦截；
   `.env.example` 与 README（部署命令注释、环境变量表）同步标注必设与 fail-fast 行为。
3. 验证：`cargo test config` 25 用例连跑 4 次全绿；`cargo fmt --check`、`cargo check`
   通过；端到端确认 `TIDE_ENV=production cargo run`（默认密钥）exit 1 并输出
   「production 环境禁止使用默认开发密钥 jwt.secret…」。
