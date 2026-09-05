# W5-5 图形验证码模块设计规格

> 日期：2026-09-05
> 状态：待用户 review
> 模块位置：W5 通用模块第 5 项（操作日志 → 登录日志 → 数据字典 → 文件上传 → **图形验证码** → 系统配置）

## 1. 背景与目标

为登录接口加图形验证码防线，语义对齐 gin-vue-admin：前端请求生成接口拿
`captcha_id + base64 PNG`，登录时带上 `captcha_id + 用户输入` 强校验。
遵守本项目既有约定：

- 统一契约：`POST + JSON body`；
- 存储/校验复用 `AppState.cache`（`utils::cache::Cache` 抽象）；
- 验证码答案不进数据库、不进 session，纯 cache 一次性消费。

## 2. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 接入范围 | **接入登录接口**：`LoginReq` 加 `captcha_id` / `captcha_value` 必填字段，登录强校验 |
| 2 | 图像生成 | **captcha crate**（新依赖）：4 位数字、180×44 PNG → base64 |
| 3 | 失效策略 | **一次性消费**：验证（无论成败）后立即删除；TTL 3 分钟（常量） |
| 4 | 验证码 id | `uuid::Uuid::new_v4()`（复用已有 uuid 依赖） |
| 5 | 存储 | `cache.set("captcha:{id}", 答案, 3min)`；key 前缀 `captcha:` 隔离 |
| 6 | 模块位置 | 新建 `src/modules/captcha/`（api/service/dto，**无 repo**——不进库） |
| 7 | 大小写 | 校验统一转小写比对（数字集无感，为未来字母集留余地） |
| 8 | 频率限制 | generate 不限频（防刷记 backlog，YAGNI） |

## 3. 接口契约

### 3.1 生成

`POST /api/v1/captcha/generate`（公开路由，不挂 AuthRequired）

```json
// 响应
{ "code": 1, "data": { "captcha_id": "a1b2c3d4-...", "image": "iVBORw0KGgo..." }, "message": "ok" }
```

- `image` 为**裸 base64** PNG 字符串（对齐 GVA 的 `b64s`），前端自行拼
  `data:image/png;base64,` 前缀；
- `captcha_id` 供登录请求回传。

### 3.2 登录请求体变更

`LoginReq` 新增两个必填字段（serde 无默认，缺字段反序列化报错）：

```json
{
  "username": "admin",
  "password": "admin123",
  "captcha_id": "a1b2c3d4-...",
  "captcha_value": "3456"
}
```

**契约变更说明**：登录响应体不变；请求体新增字段属于破坏性变更，前端登录页
必须同步改造（对接说明见 §7）。

## 4. 生成与校验流

```text
POST /captcha/generate
  → captcha crate 生成 4 位数字图像（180×44 PNG）
  → id = uuid v4
  → cache.set("captcha:{id}", 答案, 3 分钟)
  → 返回 { captcha_id, image: base64 }

login（auth::service 最先调用 captcha::service::verify）
  → cache.get("captcha:{id}") 无记录 → Biz("验证码已过期，请刷新")
  → 答案比对失败（trim + 小写）→ Biz("验证码错误")
  → 无论成败 cache.remove(key)（一次性消费，防重放）
  → 校验通过才继续原有：查用户 → 密码 → 状态 → 签发 JWT
```

- 校验先于查库：注定失败的登录不浪费 DB 查询；
- 登录日志 msg 记录「验证码错误 / 验证码已过期」，与现有失败分级一致；
- 验证码失败对外返回明确提示（不并入「用户名或密码错误」——不泄露账号信息）。

## 5. 错误处理

- 过期 / 未生成 → `AppError::Biz("验证码已过期，请刷新")`；
- 答案不匹配 → `AppError::Biz("验证码错误")`；
- captcha crate 图像编码失败 → `AppError::Internal`（理论概率极低）。

## 6. 测试策略

### 6.1 captcha service 纯单元测试（不连 MySQL）

- generate：返回可解码 base64、cache 中有对应答案；
- verify 正确答案 → 通过，且**二次验证失效**（一次性）；
- verify 错误答案 → Biz，且码同样被消费；
- verify 未知 id / 过期 → Biz(已过期)。

### 6.2 auth 集成测试更新（连 MySQL）

- 既有 login 测试因 `LoginReq` 加字段而更新：测试里 `generate` 后用
  `cache.get("captcha:{id}")` 直读答案构造请求；
- 新增：无验证码 / 错误验证码登录 → Biz，且不落「登录成功」日志。

## 7. 前端对接说明（交付物）

> 后端实现完成后，产出一份独立的前端对接说明（供前端 AI agent 使用），
> 内容至少覆盖：生成接口调用时机（登录页加载 + 点击刷新）、`<img :src>` 拼
> base64 前缀、登录请求体新增字段、错误提示文案与验证码刷新联动（收到
> 「验证码错误 / 已过期」后自动刷新图）、契约变更对现有登录调用的影响点。

## 8. 范围外

- 音频验证码、字母/汉字字符集扩展（预留 `set_chars` 能力即可）；
- generate 频率限制 / IP 防刷（backlog）；
- Redis cache 实现（Cache trait 注入点已预留）；
- 前端登录页改造（前端仓库，由对接说明驱动）。
