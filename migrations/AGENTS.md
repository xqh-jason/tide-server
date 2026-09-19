## OVERVIEW

`migrations/` — 独立 crate `migration`（edition 2021、非 workspace）：21 张表 DDL（baseline 20 + 会话表）的唯一事实来源，sea-orm-migration 1.1.0。

**已实测它可被外部 crate 依赖**（2026-09-18）：包名 `migration`，暴露 `Migrator`，
`publish = false` 只表示不发布到 crates.io，不影响 `git + tag` 依赖。

**建档理由**：得分 9（独立 crate + 独立构建路径；`Migrator` 是 schema 演进的单一入口，与主 crate 完全不同的执行方式）。

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| 迁移顺序总表 | `src/lib.rs::Migrator::migrations()` | 顺序即执行顺序，新增迁移追加在末尾 |
| 25 条历史版本占位 | `src/legacy.rs`（635 行） | 空操作，只为满足 sea-orm 版本存在性校验 |
| 全量建表基线 | `src/m20260913_000001_baseline.rs` | 20 张表最终态 DDL（含表/列注释与索引） |
| 会话表 | `src/m20260913_000002_create_sys_refresh_token.rs` | 基线之后新增的表 |
| 注释修正 | `src/m20260914_000001_fix_sys_refresh_token_revoked_by_comment.rs`、`src/m20260917_000001_fix_sys_menu_permission_comment.rs` | 纯 COMMENT 变更也走迁移（两例：`revoked_by`、`menu.permission`） |
| 列表查询索引 | `src/m20260918_000001_add_list_query_indexes.rs` | 只加等值 / 范围 / 前缀匹配真能用上的列；组合索引统一 `(deleted_at, 过滤列)`；前导通配符列不加 |
| CLI 入口 | `src/main.rs` | `cli::run_cli(migration::Migrator)` |
| sea-orm-cli 用法 | `README.md` | `migrate up / down / status / init` |

## CONVENTIONS

- 命令：本地 `up` 的标准写法以根 `AGENTS.md` 为准（必须在本目录内执行）。本目录独有的路径：`sea-orm-cli migrate up|down|status|init -d ./migrations`；CI 形式 `cd migrations && DATABASE_URL="$TIDE_DATABASE__URL" cargo run -- up && cd ..`；容器内由 `docker/entrypoint.sh` 跑 `./migration up`（`RUN_MIGRATIONS=0` 可跳过，`DATABASE_URL` 未设时回落 `TIDE_DATABASE__URL`）。
- 顺序约定：**占位在前、基线在后**——新库先空跑 25 条占位，再由 baseline 一次性建表；曾应用旧迁移的库靠「`sys_user` 是否已存在」判定跳过 baseline，新旧库 `migration up` 都可安全重入。
- 幂等护栏用 `information_schema.TABLES` 探测，不靠 `IF NOT EXISTS` 语义猜。
- 嵌套模块下 `DeriveMigrationName` 会取 `module_path` 第二段（`legacy`）导致版本号错，故 `legacy.rs` 里手写 `impl MigrationName`。
- DDL 写成原始 SQL 字符串常量（`CREATE_TABLES: &[&str]`），表间顺序无关（无外键约束）；索引命名 `uk_*` / `idx_*`；每表每列都带中文 `COMMENT`。
- `sys_site_config` 恒单行 `id=1` 由本 crate 的 `INSERT IGNORE` 保障；字典、菜单、RBAC 等业务数据种子不在这，在 `src/infra/seed.rs`。
- `sea-orm-migration` features 必须含 `runtime-tokio` 与 `sqlx-mysql`（本项目 MySQL + tokio 运行时）。
- fmt / clippy 在本 crate 要**单独再跑一遍**（CI 对根 crate 与本目录各执行一次，命令名与根目录相同但工作目录必须是 `migrations/`）。

## ANTI-PATTERNS

- 不修改 `m20260913_000001_baseline.rs`（文件头即写明「不要修改本文件」）：后续 schema 变更一律追加新迁移文件。
- 不删除 `legacy.rs`（「本文件不可删除」）：除非所有环境的 `seaql_migrations` 已清理旧版本号，否则升级会报 `migration file is missing` 拒绝启动；旧版本号残留无害。
- 不把迁移写进主 crate：根 crate 与 `migrations` 是两个独立包（非 workspace），Dockerfile 分两次构建。
- 不加物理外键约束（与全库约定一致，`src/entity/AGENTS.md` 同源）。
- 不在迁移里塞业务数据种子（除单行表 `sys_site_config`）——业务种子走应用层幂等填充。
