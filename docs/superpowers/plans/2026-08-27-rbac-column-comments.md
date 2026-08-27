# RBAC 字段注释 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 为当前 7 张 RBAC 业务表的全部字段补充中文注释。

**架构：** 新增增量迁移，不修改已执行的 W1 迁移。迁移通过 MySQL `ALTER TABLE ... MODIFY COLUMN` 补注释；字段类型、空值、默认值和自动递增保持不变。回滚时把业务字段注释清空。

**技术栈：** sea-orm-migration 1.1.x、sea-query、MySQL 8、OrbStack MySQL。

---

### 任务 1：新增 RBAC 注释迁移

**文件：**
- 创建：`migrations/src/m20260827_000002_add_rbac_column_comments.rs`
- 修改：`migrations/src/lib.rs`

- [ ] **步骤 1：注册新迁移**

```rust
mod m20260827_000002_add_rbac_column_comments;
```

在 `migrations()` 中追加：

```rust
vec![
    Box::new(m20260825_000001_create_rbac_tables::Migration),
    Box::new(m20260827_000002_add_rbac_column_comments::Migration),
]
```

- [ ] **步骤 2：实现 up 注释**

为 `sys_user`、`sys_role`、`sys_menu`、`sys_api`、`sys_user_role`、`sys_role_menu`、`sys_role_api` 全部业务列补中文注释。修改列定义时保留原类型、NOT NULL、默认值、自动递增和时间戳扩展；不重复声明主键或唯一索引，避免覆盖已建索引。

示例：

```rust
.modify_column(
    ColumnDef::new(SysUser::Username)
        .string_len(50)
        .not_null()
        .comment("登录用户名，全局唯一"),
)
```

- [ ] **步骤 3：实现 down 清空注释**

按同一列定义执行第二次修改，只把所有业务字段 `COMMENT` 设为 `""`。

- [ ] **步骤 4：临时库验证**

创建独立数据库 `salvo_vben_migration_check` 并从零执行：

```bash
cd migrations
DATABASE_URL='mysql://root:root@localhost:3307/salvo_vben_migration_check' cargo run -- up
cd ..
cargo fmt --check
cd migrations && cargo check --all-targets
```

再查询并确认 7 张表共 51 个业务字段的 `COLUMN_COMMENT != ''`：

```sql
SELECT COUNT(*) AS commented_columns
FROM information_schema.COLUMNS
WHERE TABLE_SCHEMA = 'salvo_vben_migration_check'
  AND TABLE_NAME IN ('sys_user', 'sys_role', 'sys_menu', 'sys_api',
                     'sys_user_role', 'sys_role_menu', 'sys_role_api')
  AND COLUMN_COMMENT != '';
-- 预期：51
```

验证完成后删除临时数据库。

### 任务 2：新增 RBAC 表注释迁移

**文件：**
- 创建：`migrations/src/m20260827_000003_add_rbac_table_comments.rs`
- 修改：`migrations/src/lib.rs`

- [ ] **步骤 1：注册并实现表注释**

在 `migrations()` 中追加 `m20260827_000003_add_rbac_table_comments::Migration`。第 2 个迁移已应用，因此不修改它。表注释使用 MySQL 原生 DDL：

```sql
ALTER TABLE `sys_role` COMMENT = '角色表';
```

7 张表的注释分别为：用户表、角色表、菜单与按钮权限表、后端接口权限表、用户角色关联表、角色菜单关联表、角色接口关联表。

- [ ] **步骤 2：回滚清空表注释**

`down` 对同一批表执行：

```sql
ALTER TABLE `{table}` COMMENT = '';
```

- [ ] **步骤 3：验证与应用**

在独立临时库执行 `up → down → up`；随后应用到开发库。最终断言：

```sql
SELECT COUNT(*) AS commented_tables
FROM information_schema.TABLES
WHERE TABLE_SCHEMA = DATABASE()
  AND TABLE_NAME IN ('sys_user', 'sys_role', 'sys_menu', 'sys_api',
                     'sys_user_role', 'sys_role_menu', 'sys_role_api')
  AND TABLE_COMMENT != '';
-- 预期：7
```

同时确认业务字段注释仍为 51/51。
