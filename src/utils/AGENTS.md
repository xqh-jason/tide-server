## OVERVIEW

`src/utils/` — 全局无状态工具层：错误 / 响应体 / 请求提取器 / 分页 / 人字段名称拼装 / JWT / 密码哈希 / 缓存 / 时间 / 文本——只依赖 `entity`，不依赖任何 `module`。

**建档理由**：得分 11（grep 计数，均按 次数/文件 口径：`AppError` 501 次 / 38 文件、`ApiResponse` 140 次 / 25 文件、`JsonBody` 116 次 / 20 文件、`ApiResult` 111 次 / 17 文件、`fill_user_names` 66 次 / 15 文件（`grep -ro/-rl 'X' src --include='*.rs'`）；全仓最高中心化 + 人字段协议唯一实现）。

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| 业务错误、锁冲突兜底、OpenAPI 错误体注册 | `error.rs` | `AppError` 只有 `Biz(String)` / `Internal(anyhow::Error)` 两个变体 |
| 统一响应体 `{code, data, message}` | `response.rs` | `code`：1 成功 / 0 失败 |
| `ApiResult<T>` 类型别名、13 个子模块导出 | `mod.rs` | `pub use` 了 `IdReq` / 分页三件 / `ApiResponse` |
| JSON 请求体提取与字段级中文报错 | `request.rs` | 手写 `JsonBody<T>`，非 Salvo `JsonExtractor` |
| 分页请求 / 响应 / `paginate` | `page.rs` | `PageQuery` 字段只在此定义一次，各域 DTO `#[serde(flatten)]` 内嵌 |
| 人字段 id → 显示名批量拼装 | `user_ref.rs` | 全项目唯一管道，见下 |
| `status` 等通用小校验 | `check.rs` | `check_status` / `collect_missing_ids` / `duplicate_ids` / `format_ids` |
| 列表时间范围入参解析 | `datetime.rs` | `parse_datetime(field, s, end_of_day)` |
| 响应体时间字段序列化（非日志） | `serde_format.rs` | `naive_datetime` 输出 `%Y-%m-%d %H:%M:%S`，非 serde ISO |
| 密码 argon2id、通用 SHA-256 | `crypt.rs` | refresh token 落库存 SHA-256 hex |
| JWT 签发 / 校验（HS256） | `jwt.rs` | `Claims` / `sign` / `verify`；载荷含 `refresh_token_id`，缺该字段的旧 token 反序列化即失败——发版后全员重登即迁移路径，无双轨兼容 |
| 缓存抽象与内存实现 | `cache.rs` | `Cache` trait + `MemoryCache`，`Arc<dyn Cache>` 注入 `AppState` |
| 单 id 请求体 | `id_req.rs` | `IdReq`，所有域共用 |
| 字符安全截断 | `text.rs` | `truncate_chars` |

## CONVENTIONS

- **人字段命名**：指向 `sys_user` 的引用字段一律 `动词过去式_by`（`created_by` / `approved_by` / `submitted_by` / `assigned_by`），不与 `xxx_id` 混用；列类型 `BIGINT UNSIGNED NOT NULL DEFAULT 0`（`0` = 种子 / 系统写入），实体与 Resp 中均为 `u64` 非 `Option`。
- **名称字段**：Resp 中 = 人字段名 + `_name`（`created_by_name`），后端批量拼装返回，前端不做 id → 名称换算；查不到显示名给空串，由前端渲染占位符。
- **拼装协议**（单一实现，只依赖 entity）：实体 impl `UserRefIds::user_ref_ids()` 收集本记录全部人字段 id；Resp impl `UserRefNames::set_user_ref_names()` 填对应 `*_name`；端点一行 `fill_user_names(db, items, Resp::from)`（收集 → `dedup_ids` → 一次 `IN` 批量查 → 填充）。显示名取 `username`，**不排除软删**——名称解析面向历史引用，操作人即便已软删，历史记录仍应带出名字。
- 新增人字段 = 实体 impl 加一个 id + Resp 加一对字段，协议与管道零改动。
- **写入口径**：审计字段由 repo 层统一盖章（create 双写 `created_by` / `updated_by`，update 只刷 `updated_by`、`created_by` 保持 `NotSet`），service 只透传 `actor_id`；请求体不接受人字段，防伪造。现状已落地：`created_by` / `updated_by` / `revoked_by`（本人登出即本人 id）。
- 错误收口在 `From<anyhow::Error> for AppError`（手写而非 thiserror `#[from]`）：MySQL **1213 死锁 / 1205 锁等待超时** → `Biz("操作冲突，请稍后重试")`，其余一律 `Internal`。识别用 `MySqlDatabaseError::number()` 而非 `code()`（SQLSTATE `40001` / `HY000` 区分不出两者），并覆盖 `Conn` / `Exec` / `Query` 三处（死锁也可能出现在 commit）。加锁读的规则本体见 `src/modules/system/AGENTS.md`。
- `Internal` 对外只回固定串 `internal error`（不泄漏 SQL / 连接串），底层错误必须 `tracing::error!` 落日志。
- `EndpointOutRegister for AppError` 检测到已有 `"200"` 响应即 early return，否则 Err 体会覆盖 Ok 体、Swagger 里看不到 `data` 类型。
- `page_size` 越界不拒绝而是 `clamp(1, 1000)`；`page_index()` 把 1-based 转 0-based。
- `MemoryCache` 有容量上限 `DEFAULT_CAPACITY = 100_000`（防公开端点如验证码以随机 key 无限写入吃内存），满时先清过期再驱逐最早过期条目。
- 本目录以**纯单元测试**为主（不连库）；例外是 `user_ref.rs`，其拼装管道用真库 fixture 验证。

## ANTI-PATTERNS

- `utils` 不得 `use crate::modules::*`：会形成环，人字段协议因此只依赖 `entity`（生产代码零 modules 依赖；仅 `#[cfg(test)]` 例外，先例：`jwt.rs` 测试用 `SUPER_ROLE_KEY`）。
- 不新增第二条名称拼装管道，不在各域自己 JOIN `sys_user` 取显示名。
- 值域校验不写进 DTO、也不写进本目录的通用工具：请求体校验在各域私有 `validate.rs`，`check_status` 的允许值由调用方从字典读后传入（**不是硬编码**）。
- 不改 `JsonBody` 为 Salvo 内置提取器：`affix-state` 依赖 salvo_extra 0.95.2、rsproxy 镜像暂无，故手写等价实现；`#[serde(flatten)]` 会丢字段路径，错误提示靠 serde_path_to_error + `FIELD_LABELS`，且只回传值是什么类型、不回传值本身。
- `datetime.rs` 里保留的错误分支只为规避 `expect_used = "deny"`，不要顺手改成 `expect`。
