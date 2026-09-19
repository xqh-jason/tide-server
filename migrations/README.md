# 数据库迁移（sea-orm-migration）

## 常用命令（sea-orm-cli，版本与根 crate 的 sea-orm 依赖同主版本）

```bash
sea-orm-cli migrate up      # 应用所有待执行迁移
sea-orm-cli migrate down    # 回滚最近一次
sea-orm-cli migrate status  # 查看迁移状态
sea-orm-cli migrate init    # 初始化迁移工程（-d 指定目录）
```

> 注意：本项目迁移工程位于 `migrations/` 目录（`-d ./migrations` 初始化）。
> 本项目用 MySQL + tokio 运行时，features 需含 `runtime-tokio` 与 `sqlx-mysql`。

本地执行（不用 sea-orm-cli 时）：

```bash
cd migrations
DATABASE_URL='mysql://root:root@localhost:3307/tide_server?charset=utf8mb4&timezone=%2B08:00' cargo run -- up
```

表结构 = `src/m20260913_000001_baseline.rs`（基线迁移）**加上其后的增量迁移**
（登记在 `src/lib.rs` 的 `Migrator`）。改表一律**追加新迁移文件**，不改已发布的 baseline；
平台表的相对顺序不能变。
