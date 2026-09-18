# 贡献指南

感谢愿意参与。本仓库是一个 Rust 学习型项目 + 生产形态的 RBAC 后端，代码风格与约束偏严格，
提 PR 前请先读完下面几节——大部分「被拒」的原因都写在里面。

## 快速开始

需要 **Rust 1.96.0**（`rust-toolchain.toml` 已钉住）与 **MySQL 8**。本机已有 MySQL 可跳过第 1 步。

```bash
# 1. 起开发库（宿主 3307 → 容器 3306，首次自动建库）
docker compose up -d mysql

# 2. 建表（迁移是独立 crate，必须在 migrations/ 目录执行；根目录 cargo run 是启动服务）
cd migrations
DATABASE_URL='mysql://root:root@localhost:3307/tide_server' cargo run -- up
cd ..

# 3. 跑起来（development 档位自动执行幂等种子）
cargo run

# 4. 跑测试（需上面的 MySQL 在跑）
cargo test
```

默认账号 `admin / admin123`（development 档位每次启动重置，仅限本地）。

## 作为基座开发（两个仓库）

本仓是**基座**，不包含业务表。两种贡献场景不要搞混：

| 改造目标 | 改哪里 | 说明 |
|---|---|---|
| 平台能力（RBAC / 认证 / 字典 / 日志 / 任务 / 文件 / 组织） | **本仓** | 改完要走下面的发版流程 |
| 具体业务（人事 / ERP / …） | **业务仓**（如 tide-hr） | 不在本仓提 PR |

本仓对外接口就是 `src/lib.rs` 里的 6 个 `pub mod`。**改动公开面（新增/修改 pub 项、
改变函数签名）属于破坏性变更**：业务仓以 git 依赖 + 版本 tag 消费本仓，签名一改它们
就编不过。这类改动请同时说明升级影响。

### 发版（tag）

业务仓靠 tag 固定版本，所以 tag 即发布点。**门禁全绿后再打 tag**：

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git tag v0.2.1 && git push origin main --tags
```

CI 已监听 `tags: ["v*"]`；但 CI 是事后发现，本地先跑一轮能省一个来回
（2026-09-18 实际踩过：tag 打好后才发现该提交有 clippy 告警，只能删 tag 重打）。

## 提交前必跑

CI 会跑同样的命令，本地先过一遍可以省一个来回：

```bash
cargo fmt --check      && (cd migrations && cargo fmt --check)
cargo clippy --all-targets -- -D warnings && (cd migrations && cargo clippy --all-targets -- -D warnings)
cargo test
```

三件事请注意：

- **clippy 是 `-D warnings`**，任何 warning 都会让 CI 红；`unsafe_code` / `unwrap_used` /
  `expect_used` / `dbg_macro` / `todo` 在非测试代码里是 `deny` 级。
- **测试连真库、无 mock**，夹具用 `test_txn()` 事务回滚隔离。不要在测试里写死 sleep 或依赖
  执行顺序。
- **同时跑两个 `cargo test` 会互相干扰**（共享种子行），全量请串行。
- **全量 `cargo test` 会同时跑 lib 与 bin 两个 target 的测试**；只关心库侧时用 `cargo test --lib`。

## 代码约定（摘要）

完整版在 [AGENTS.md](AGENTS.md) 与各目录的 `AGENTS.md`（`src/`、`src/infra/`、
`src/modules/`、`src/modules/system/`、`src/utils/`、`src/middleware/`、`src/entity/`）。
给人类读者的要点：

- **分层方向固定**：`api → service → repo → entity`；展示类查询不要顺手加锁，
  「读 → 判断 → 写」的不变式才用 `SELECT ... FOR UPDATE`。
- **repo 只拼 SQL**：不在 repo 层做业务保留字判断、唯一性决策、跨域调用。
- **软删除**：主表过滤 `deleted_at IS NULL`，关系表硬删除。
- **注释用简体中文，中英文之间留空格**；标识符用英文。
- **请求体不接受审计字段**（`created_by` 等），由 repo 层统一盖章，防伪造。

## 接口契约

改动端点时注意这是**对外契约**，README 的「接口契约」一节与前端对接说明需要同步更新：

- 一律 `POST + JSON`（文件上传 multipart；`file/download`、`site-config/get` 为 GET）
- 响应体恒为 `{ code, data, message }`，`code=1` 成功 / `0` 失败，**HTTP 恒 200**，
  唯一例外是认证失败 401
- 新增管理端点需在 `src/infra/seed.rs` 的 `API_SEEDS` 登记，否则接口授权对它是
  **fail-open**（未登记即放行）

## 提交信息与 PR

提交信息遵循 [Conventional Commits](https://www.conventionalcommits.org/)，**描述用中文**：

```
feat(rbac): 完善软删除过滤与用户创建校验
fix(pagination): 分页查询统一按主键降序，消除跨页重复与漏项
```

类型限 `feat` / `fix` / `refactor` / `chore` / `docs` / `test`。

PR 里请写明：改动目的、验证证据（`cargo test` 的实际输出或结论）、契约变更（响应体或端点）
以及对应的前端影响。涉及表结构请在 `migrations/` 新增迁移文件，不要修改已发布的 baseline。

## 安全问题

**不要开公开 Issue**，走 [SECURITY.md](SECURITY.md) 里的私下报告通道。
