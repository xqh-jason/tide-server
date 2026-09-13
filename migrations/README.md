# 数据库迁移（sea-orm-migration）

## 常用命令（sea-orm-cli 1.1.x，与 sea-orm 1.1.x 对齐）

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
DATABASE_URL='mysql://root:root@localhost:3307/tide_server' cargo run -- up
```

完整表结构以 `src/m20260913_000001_baseline.rs`（基线迁移）为准。
