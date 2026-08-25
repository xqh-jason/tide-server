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

## W1 需要定稿的表结构（5 张核心表 + 3 张关联表）

对照 GVA `server/initialize/` 目录设计，字段先抄 GVA 再按需精简：

- `sys_user`        用户（含密码哈希、状态、角色关联）
- `sys_role`        角色
- `sys_menu`        菜单/按钮（含 path/name/component/meta/permission 码）
- `sys_api`         接口（路径 + 方法，供权限点登记）
- `sys_user_role`   用户-角色 关联
- `sys_role_menu`   角色-菜单 关联
- `sys_role_api`    角色-接口 关联

> 注意：菜单 `permission` 码是「前端权限码 + 后端授权点」的唯一事实来源（见计划 §3.5）。
