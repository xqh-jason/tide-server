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
