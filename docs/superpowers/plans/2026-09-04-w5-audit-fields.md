# 创建人 / 更新人审计字段（W5 补项）实现计划

> **面向 AI 代理的工作者：** 本计划遵循项目协作约定（W3 起固定分工）：
> AI 编写失败测试与脚手架、做最终 review 与验证；用户手动实现业务代码。
> 步骤中用 **[AI]** / **[用户]** 标注执行者，复选框跟踪进度。

**目标：** 为 6 张由人工维护的配置类主表（`sys_user`、`sys_role`、`sys_menu`、
`sys_api`、`sys_dictionary`、`sys_dictionary_detail`）各加 `created_by` /
`updated_by` 两个审计字段，存 `user_id`（`BIGINT UNSIGNED NULL`），
并在所有走 repo 的写入路径上自动盖章。

**不在范围内：** 日志表（`sys_operation_log` / `sys_login_log` 已有 `user_id`，语义重复）、
3 张纯关联表（硬删除、无审计价值）、`deleted_by`（软删由 `deleted_at` + 操作日志覆盖）。

**规格依据：** 用户 2026-09-04 决策——6 张配置主表加双字段、存 `user_id`。

> **收尾补记（2026-09-04）**：经用户授权「全部你来加」，本轮由 AI 全程实现
> （替代既有 AI 测试 / 用户实现的分工）。主项目 147/147、codegen 16/16 全绿。
> 实施偏差记录：
> - 模板过滤实际只需改 Resp 两处——`readonly: true` 已天然把 audit 字段
>   挡在 Create/Update/seed 之外，`audit` 标记只负责"进 Resp"；
> - `sys_dictionary*.rs` 重跑生成器验证时发现生成器仍会把 `type` 写成裸
>   标识符（既有缺陷），已手工修回 `r#type` 并保留 audit 字段；
> - 既有测试统一以 `const ACTOR_ID: u64 = 1`（种子 admin）作操作人。
>
> **二次调整（同日，用户反馈）**：字段弃用 `Option<u64>`，改为
> `BIGINT UNSIGNED NOT NULL DEFAULT 0`（`0` = 种子/系统写入，无操作人上下文）；
> 实体、Resp、codegen defs 同步为 `u64`。迁移 `000010` 尚未推送，直接修正
> 文件列定义并手动同步本地库（迁移记录已标记应用）。create 双写行为不变。

---

## 关键设计决策

| # | 决策点 | 结论 | 理由 |
|---|---|---|---|
| 1 | 字段类型 | `BIGINT UNSIGNED NULL DEFAULT NULL`，实体为 `Option<u64>` | 与 `sys_operation_log.user_id` 先例一致；不冗余 `username`（可改名，会失同步） |
| 2 | 注入位置 | **repo 层统一注入**，service 只透传 `actor_id` | 全部业务写入都走 repo（`seed.rs` 除外），集中一处不会漏设；`updated_by` 没有 MySQL 的 `ON UPDATE` 等价物，漏一处就是脏数据 |
| 3 | 存量数据 | 全部 `NULL`，不回填、不硬编 0 | 迁移前的数据没有可信操作人 |
| 4 | 种子数据 | 保持 `NULL`，`seed.rs` 不改 | `seed.rs` 用 `ActiveModel{...}.insert(db)` 直接写，不走 repo，加列后天然不受影响 |
| 5 | 软删除 | 不写 `updated_by` | 记录随即不可见，且操作日志已覆盖删除行为 |
| 6 | DTO | Resp 增加 `created_by` / `updated_by` 两个 `Option<u64>`；Create/Update 请求体**不接受**这两个字段 | 审计字段只能由系统写入，不接受前端传参 |
| 7 | 列表展示 | 只返回 `user_id`，不做昵称回查 | 按用户选择的最小改动；前端需要昵称时另起一轮（批量查 `sys_user` 拼装） |

### 为什么不在 service 层构造 ActiveModel 时赋值

现有 service 构造 ActiveModel 均带 `..Default::default()`，追加两行 `Set(...)` 看似简单，
但 6 个域 11 个 create / update 函数都要改，且未来新增域时极易漏。
repo 层注入把"哪些写入需要盖章"压缩成 11 个函数签名，漏设会在编译期暴露。

### 调用链全景（改后）

```text
handler（api.rs）  AuthUser::from_depot(depot)?  → auth.user_id
  → service(db, actor_id, req)                    业务规则 + 构造 ActiveModel（不碰审计字段）
    → repo(db, ..., actor_id)                     model.created_by / updated_by = Set(...)
      → insert / update
```

---

## 文件结构

创建：
- `migrations/src/m20260904_000010_add_audit_columns.rs`

修改：
- `migrations/src/lib.rs`：注册迁移
- `src/entity/{sys_user,sys_role,sys_menu,sys_api,sys_dictionary,sys_dictionary_detail}.rs`：各加 2 字段
- `src/modules/{user,role,menu,sys_api,dictionary}/repo.rs`：create / update 加 `actor_id` 并注入
- `src/modules/{user,role,menu,sys_api,dictionary}/service.rs`：透传 `actor_id`
- `src/modules/{user,role,menu,sys_api,dictionary}/api.rs`：取 `AuthUser` 并传 `auth.user_id`
- `src/modules/{user,role,menu,sys_api,dictionary}/dto.rs`：Resp 加 2 字段
- `codegen/src/def.rs`、`codegen/src/templates.rs`：支持 `audit` 字段语义
- `codegen/defs/dictionary.json`、`codegen/defs/dictionary_detail.json`：加 2 字段

测试（AI 编写，写入各文件 `#[cfg(test)]`）：
- 5 个域 `repo.rs`：审计盖章断言（12 个用例）
- `codegen/src/templates.rs`：`audit` 语义单测

---

## 任务 1：迁移加列（[用户]）

**文件：** `migrations/src/m20260904_000010_add_audit_columns.rs`、`migrations/src/lib.rs`

- [x] **步骤 1：编写迁移文件**

一次迁移改 6 张表，每张表加两列并补列注释（对齐
`m20260827_000002_add_rbac_column_comments.rs` 的注释写法）：

```rust
//! W5 迁移：为 6 张配置类主表补 created_by / updated_by 审计字段。
//!
//! 存 sys_user.id，NULL 表示无操作人上下文（种子数据、存量数据与迁移前记录）。

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let tables: [(&str, &str); 6] = [
            ("sys_user", "用户"),
            ("sys_role", "角色"),
            ("sys_menu", "菜单"),
            ("sys_api", "接口"),
            ("sys_dictionary", "字典类型"),
            ("sys_dictionary_detail", "字典项"),
        ];

        for (table, label) in tables {
            // 加列：ALTER TABLE ... ADD COLUMN ... （MySQL 单条 ALTER 支持多 ADD）
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new(table))
                        .add_column(
                            ColumnDef::new(Alias::new("created_by"))
                                .big_unsigned()
                                .null(),
                        )
                        .add_column(
                            ColumnDef::new(Alias::new("updated_by"))
                                .big_unsigned()
                                .null(),
                        )
                        .to_owned(),
                )
                .await?;

            // 列注释（MySQL 用 MODIFY COLUMN 追加 COMMENT，保持与原列定义一致）
            let db = manager.get_connection();
            db.execute_unprepared(&format!(
                "ALTER TABLE `{table}` \
                   MODIFY COLUMN `created_by` BIGINT UNSIGNED NULL DEFAULT NULL COMMENT '创建人 ID', \
                   MODIFY COLUMN `updated_by` BIGINT UNSIGNED NULL DEFAULT NULL COMMENT '更新人 ID'"
            ))
            .await?;
            let _ = label; // 注释中的业务名已在 COMMENT 体现
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "sys_user",
            "sys_role",
            "sys_menu",
            "sys_api",
            "sys_dictionary",
            "sys_dictionary_detail",
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new(table))
                        .drop_column(Alias::new("updated_by"))
                        .drop_column(Alias::new("created_by"))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
```

说明：`Table::alter().add_column()` 两次在 SeaORM 生成的 SQL 中为
`ALTER TABLE ... ADD COLUMN ... , ADD COLUMN ...`，MySQL 8 支持；
注释必须走 `MODIFY COLUMN`，因为 SeaORM 的 `add_column` 不产生 `COMMENT`。

- [x] **步骤 2：注册迁移**

`migrations/src/lib.rs` 追加 `mod m20260904_000010_add_audit_columns;`，
并在 `Migrator::migrations()` 末尾追加
`Box::new(m20260904_000010_add_audit_columns::Migration),`。

- [x] **步骤 3：应用迁移并验证**

```bash
cd migrations && DATABASE_URL='mysql://root:root@localhost:3307/salvo_vben' cargo run -- up
docker exec salvo-vben-mysql mysql -uroot -proot salvo_vben --default-character-set=utf8mb4 \
  -e "SELECT TABLE_NAME, COLUMN_NAME, IS_NULLABLE, DATA_TYPE, COLUMN_COMMENT
      FROM information_schema.COLUMNS
      WHERE TABLE_SCHEMA='salvo_vben' AND COLUMN_NAME IN ('created_by','updated_by')
      ORDER BY TABLE_NAME, COLUMN_NAME"
```

预期：6 张表各 2 列，共 12 行；`IS_NULLABLE = YES`、`DATA_TYPE = bigint`、
注释分别为「创建人 ID」「更新人 ID」。

- [x] **步骤 4：Commit（[用户]）**

```bash
git add migrations/src/m20260904_000010_add_audit_columns.rs migrations/src/lib.rs
git commit -m "feat(migrations): 配置类主表补充创建人/更新人审计字段"
```

---

## 任务 2：实体加字段（[用户]）

**文件：** `src/entity/` 下 6 个实体

- [x] **步骤 1：`sys_user` / `sys_role` / `sys_menu` / `sys_api`（手写实体）**

在每份实体的 `deleted_at` 之前插入两个字段（保持"审计三件套 + 软删"相邻）：

```rust
    /// 创建人 ID（sys_user.id；种子与存量数据为 NULL）
    pub created_by: Option<u64>,
    /// 更新人 ID（sys_user.id）
    pub updated_by: Option<u64>,
```

- [x] **步骤 2：`sys_dictionary` / `sys_dictionary_detail`（codegen 实体）**

改 `codegen/defs/dictionary.json` 与 `dictionary_detail.json`，
在 `deleted_at` 之前插入（任务 6 会同步模板，本步先手写 entity 保证主项目可编译）：

```json
    { "name": "created_by", "rust_type": "Option<u64>", "sql_type": "BIGINT UNSIGNED", "readonly": true, "audit": true, "comment": "创建人 ID" },
    { "name": "updated_by", "rust_type": "Option<u64>", "sql_type": "BIGINT UNSIGNED", "readonly": true, "audit": true, "comment": "更新人 ID" },
```

本步直接在 `src/entity/sys_dictionary*.rs` 手改为 `Option<u64>` 字段，
任务 6 完成后重跑生成器，确认产出的 entity 与手写结果一致（无 diff）。

- [x] **步骤 3：验证编译**

```bash
cargo check
```

预期：无新增错误。加列不破坏现有代码——`ActiveModel` 未 `Set` 的字段是 `NotSet`，
不进 SQL，现有 90+ 处构造点不受影响。

- [x] **步骤 4：Commit（[用户]）**

```bash
git add src/entity/
git commit -m "feat(entity): 6 张主表实体补充 created_by / updated_by"
```

---

## 任务 3：repo 层统一注入（[AI] 测试 → [用户] 实现）

**文件：** `src/modules/{user,role,menu,sys_api,dictionary}/repo.rs`

- [x] **步骤 1：AI 写入失败测试（每域 2 个，共 12 个）**

统一命名 `<行为>_<场景>`，测试夹具沿用各域既有 helper
（唯一命名 `<prefix>_<pid>_<seq>`，测后先删关联表再删主表）：

```rust
// —— user/repo.rs ——
#[tokio::test]
async fn create_user_with_links_stamps_actor_as_creator_and_updater() {}
// seed_actor 造操作人 a → create_user_with_links(db, model, a.id, vec![])
// 断言 created_by == Some(a.id) && updated_by == Some(a.id)

#[tokio::test]
async fn update_user_with_links_refreshes_updated_by_and_keeps_created_by() {}
// 先以 a 创建（created_by = a）→ 再以 b 更新
// 断言 updated_by == Some(b.id) && created_by == Some(a.id)（不被覆盖）

// —— role/repo.rs ——
#[tokio::test]
async fn create_role_with_links_stamps_actor_as_creator_and_updater() {}
#[tokio::test]
async fn update_role_with_links_refreshes_updated_by_and_keeps_created_by() {}
// update_role（状态更新通用函数）同样要盖章：
#[tokio::test]
async fn update_role_refreshes_updated_by_for_status_change() {}

// —— menu/repo.rs ——
#[tokio::test]
async fn create_menu_stamps_actor_as_creator_and_updater() {}
#[tokio::test]
async fn update_menu_refreshes_updated_by_and_keeps_created_by() {}

// —— sys_api/repo.rs ——
#[tokio::test]
async fn create_api_with_links_stamps_actor_as_creator_and_updater() {}
#[tokio::test]
async fn update_api_with_links_refreshes_updated_by_and_keeps_created_by() {}

// —— dictionary/repo.rs ——
#[tokio::test]
async fn create_dictionary_stamps_actor_as_creator_and_updater() {}
#[tokio::test]
async fn update_dictionary_refreshes_updated_by_and_keeps_created_by() {}
#[tokio::test]
async fn create_detail_stamps_actor_as_creator_and_updater() {}
#[tokio::test]
async fn update_detail_refreshes_updated_by_and_keeps_created_by() {}
```

- [x] **步骤 2：运行确认红（[用户]）**

```bash
cargo test audit 2>&1 | tail -20   # 或按域：cargo test role::repo
```

预期：编译失败——repo 函数还没有 `actor_id` 参数。

- [x] **步骤 3：用户改造 11 个 repo 函数**

签名变化（`actor_id: u64` 统一放最后，与既有可变参数顺序一致）：

```rust
// user/repo.rs
pub async fn create_user_with_links(db, user: sys_user::ActiveModel, role_ids: Vec<u64>, actor_id: u64)
pub async fn update_user_with_links(db, user: sys_user::ActiveModel, role_ids: Vec<u64>, actor_id: u64)
pub async fn update_user(db, model: sys_user::ActiveModel, actor_id: u64)

// role/repo.rs
pub async fn create_role_with_links(db, role, menu_ids, api_ids, actor_id: u64)
pub async fn update_role_with_links(db, role, menu_ids, api_ids, actor_id: u64)
pub async fn update_role(db, role, actor_id: u64)

// menu/repo.rs
pub async fn create_menu(db, model, actor_id: u64)
pub async fn update_menu(db, model, actor_id: u64)

// sys_api/repo.rs
pub async fn create_api_with_links(db, api, role_ids, actor_id: u64)
pub async fn update_api_with_links(db, api, role_ids, actor_id: u64)

// dictionary/repo.rs
pub async fn create_dictionary(db, model, actor_id: u64)
pub async fn update_dictionary(db, model, actor_id: u64)
pub async fn create_detail(db, model, actor_id: u64)
pub async fn update_detail(db, model, actor_id: u64)
```

注入写法（create 与 update 各两行，放在 `insert` / `update` 调用之前）：

```rust
// create：创建人与更新人同源
let mut user = user;
user.created_by = Set(Some(actor_id));
user.updated_by = Set(Some(actor_id));
let user = user.insert(&txn).await?;

// update：只刷新更新人；created_by 保持 NotSet，不会被覆盖
let mut user = user;
user.updated_by = Set(Some(actor_id));
let user = user.update(&txn).await?;
```

需要 `use sea_orm::ActiveValue::Set;`（各 repo 文件已有导入）。
注意 `create_*_with_links` 里参数需改为 `mut` 绑定才能赋值。

- [x] **步骤 4：运行测试验证绿（[用户]）**

```bash
cargo test user::repo role::repo menu::repo sys_api::repo dictionary::repo 2>&1 | tail -20
```

- [x] **步骤 5：Commit（[用户]）**

```bash
git add src/modules/
git commit -m "feat(repo): 写入统一注入创建人/更新人审计字段"
```

---

## 任务 4：service 层透传 actor（[用户]）

**文件：** 5 个域的 `service.rs`

- [x] **步骤 1：改签名并透传**

现状与改法：

| 域 | 函数 | 现状 | 改法 |
|---|---|---|---|
| user | `create_user` | 已有 `actor_id`（权限校验用） | 直接透传给 repo |
| user | `update_user_with_links` | 已有 `actor_id` | 直接透传给 repo |
| user | `update_user_status` | `(db, id, status)` | 加 `actor_id: u64`，透传给 `repo::update_user` |
| role | `create_role` | `(db, req)` | 加 `actor_id: u64` |
| role | `update_role` | `(db, req)` | 加 `actor_id: u64` |
| role | `update_role_status` | `(db, req)` | 加 `actor_id: u64` |
| menu | `create_menu` | `(db, req)` | 加 `actor_id: u64` |
| menu | `update_menu` | `(db, req)` | 加 `actor_id: u64` |
| sys_api | `create_api` | `(db, req)` | 加 `actor_id: u64` |
| sys_api | `update_api` | `(db, req)` | 加 `actor_id: u64` |
| dictionary | `create_dictionary` | `(db, req)` | 加 `actor_id: u64` |
| dictionary | `update_dictionary` | `(db, req)` | 加 `actor_id: u64` |
| dictionary | `create_dictionary_detail` | `(db, req)` | 加 `actor_id: u64` |
| dictionary | `update_dictionary_detail` | `(db, req)` | 加 `actor_id: u64` |

参数位置：`actor_id` 统一紧跟 `db` 之后（与 `create_user(db, actor_id, req)` 既有风格一致）。
service 内部构造的 ActiveModel **不要**设置审计字段，交给 repo。

- [x] **步骤 2：验证编译（预期大量调用点报错）**

```bash
cargo check 2>&1 | grep -E "^error" | head -40
```

报错集中在两部分：api.rs 调用点（任务 5 修）与测试调用点（步骤 3 修）。

- [x] **步骤 3：同步修正测试调用点**

各域 `service.rs` / `repo.rs` 的 `#[cfg(test)]` 内直接调 service 的用例，
统一补一个 actor 实参——复用各域既有的 `seed_actor` / `load_or_create_admin` 夹具，
或直接用 `load_or_create_admin(db).await.0.id`。
`user/service.rs` 的 `update_user_status` 用例同理。

- [x] **步骤 4：全量测试**

```bash
cargo test 2>&1 | tail -20
```

- [x] **步骤 5：Commit（[用户]）**

```bash
git add src/modules/
git commit -m "refactor(service): 透传 actor_id 以支撑审计字段写入"
```

---

## 任务 5：api 层取当前用户（[用户]）

**文件：** 5 个域的 `api.rs`

- [x] **步骤 1：补 `AuthUser` 提取**

`user/api.rs` 的 `create_user` / `update_user` 已取 `auth`（权限校验用），
只需给 `update_user_status`（第 90 行起）补一行。
其余 4 个域的 create / update / status handler 需新增两行：

```rust
use crate::middleware::auth::AuthUser;   // role / sys_api / dictionary 三个文件需新增导入
...
let auth = AuthUser::from_depot(depot)?;
let resp = role_service::create_role(&state.db, auth.user_id, &req).await?;
```

需改的 handler 清单（11 个）：

- `role/api.rs`：`create_role`(28)、`update_role`(37)、`update_role_status`(46)
- `sys_api/api.rs`：`create_api`(29)、`update_api`(38)
- `dictionary/api.rs`：`create_dictionary`(35)、`update_dictionary`(47)、
  `create_dictionary_detail`(108)、`update_dictionary_detail`(120)
- `menu/api.rs`：`create_menu`(41)、`update_menu`(50)
- `user/api.rs`：`update_user_status`(90)

前提确认：这些路由组在 `src/infra/router.rs` 均已 `.hoop(AuthRequired)`
（第 17–68 行已核对），`AuthUser::from_depot` 必然取到值。

- [x] **步骤 2：DTO Resp 加字段**

5 个域 dto.rs 的 Resp 结构体各加两个字段，并在 `From<Model>` 里搬运：

```rust
pub struct RoleResp {
    // ...既有字段
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
}
```

涉及：`UserResp`、`RoleResp`、`MenuResp`、`ApiResp`、`DictionaryResp`、
`DictionaryDetailResp`。Create / Update 请求结构体**不加**（审计字段不接受前端传参）。

- [x] **步骤 3：验证编译与测试**

```bash
cargo fmt --check
cargo check
cargo test 2>&1 | tail -20
```

- [x] **步骤 4：Commit（[用户]）**

```bash
git add src/modules/
git commit -m "feat(api): create/update 端点注入当前用户并回传审计字段"
```

---

## 任务 6：codegen 支持审计字段（[AI] 模板单测 → [用户] 实现）

**文件：** `codegen/src/def.rs`、`codegen/src/templates.rs`、`codegen/defs/dictionary*.json`

> 本轮做这件事的理由：W5 还剩文件上传、系统配置、部门、岗位 4 个模块要走生成器，
> 模板不认审计字段，每个新模块都要手工补一遍。

- [x] **步骤 1：AI 追加模板单测**

`codegen/src/templates.rs` 的测试模块追加（沿用既有 fixture 写法）：

```rust
#[test]
fn audit_fields_appear_in_resp_but_not_in_create_or_update_req() {}
// 造一个含 created_by/updated_by（audit: true）的 def：
//   - entity 源码含 `pub created_by: Option<u64>`
//   - Resp 结构体含 created_by / updated_by
//   - CreateReq / UpdateReq 不含这两个字段
//   - 生成的 Create / Update ActiveModel 不含 created_by / updated_by 的 Set

#[test]
fn audit_fields_excluded_from_seed_active_model() {}
// 生成的测试 seed 不为审计字段赋值
```

- [x] **步骤 2：运行确认红（[用户]）**

```bash
cd codegen && cargo test 2>&1 | tail -20
```

- [x] **步骤 3：实现（[用户]）**

`def.rs` 的 `FieldDef` 加一个标记：

```rust
    /// 审计字段：由系统（repo 层）写入，不进创建/更新请求体，但要进响应体。
    #[serde(default)]
    pub audit: bool,
```

`templates.rs` 改 5 处过滤（行号为改动前位置，仅作定位参考）：

| 位置 | 现在 | 改为 |
|---|---|---|
| ~318 `seed` ActiveModel 字段 | `!f.primary && !f.readonly && !exclude` | 追加 `&& !f.audit` |
| ~480 Create ActiveModel 字段 | `!f.primary && !f.readonly` | 追加 `&& !f.audit` |
| ~514 Update ActiveModel 字段 | `!f.primary && !f.readonly` | 追加 `&& !f.audit` |
| ~850/~863 Resp 字段 | `f.primary \|\| !f.readonly` | 改为 `f.primary \|\| f.audit \|\| !f.readonly` |
| ~900/~916 Create / Update 请求字段 | `!f.primary && !f.readonly` | 追加 `&& !f.audit` |

`typing.rs` 无需改动：`entity_type` 对 `Option<u64>` 走 `other` 分支原样返回。

- [x] **步骤 4：同步两个 defs 并回归生成（[用户]）**

```bash
cd codegen && cargo test 2>&1 | tail -10
cd codegen && cargo run -- generate ../codegen/defs/dictionary.json
cd codegen && cargo run -- generate ../codegen/defs/dictionary_detail.json
```

生成器会连带输出 `src/modules/dictionary*/` 四件套——**不要覆盖**现有手写域，
确认 `src/entity/sys_dictionary*.rs` 与任务 2 手写结果一致后，
把生成出的 modules 目录丢弃（`git checkout -- src/modules/` 或删除）。

- [x] **步骤 5：Commit（[用户]）**

```bash
git add codegen/
git commit -m "feat(codegen): 支持 audit 审计字段语义并同步字典域定义"
```

---

## 任务 7：全量验证 + Review + 打卡（[AI]）

- [x] **步骤 1：全量验证**

```bash
cargo fmt --check
cargo check
cargo test 2>&1 | tail -20
cd codegen && cargo test 2>&1 | tail -5
```

预期：双项目全绿，格式无 diff。

- [x] **步骤 2：AI Review**

逐条核对：

1. 6 张表列存在且可空、注释正确（12 行）；
2. 11 个 repo 函数全部注入，无遗漏（对比任务 3 清单逐一 grep `Set(Some(actor_id))`）；
3. `create_*` 设置 created_by + updated_by，`update_*` 只设置 updated_by；
4. `created_by` 在 update 后不被覆盖（任务 3 的 `keeps_created_by` 用例覆盖）；
5. 11 个 handler 全部传 `auth.user_id`，无遗漏；
6. Create / Update 请求结构体未混入审计字段；
7. `seed.rs` 未被改动，种子数据审计字段为 NULL；
8. 日志表与 3 张关联表未被波及；
9. codegen 重新生成的 entity 与仓库内一致。

发现问题按「必须修复 / 建议修改」分级反馈，由 [用户] 修正后回到步骤 1 重跑。

- [x] **步骤 3：手动冒烟（可选，[用户]）**

启动服务 → 用 admin 登录 → 新建一个角色 → 查 `role/get` 确认
`created_by` / `updated_by` 为 admin 的 `id` → 换一个账号编辑该角色 →
确认 `updated_by` 变更而 `created_by` 不变。

- [x] **步骤 4：更新打卡记录（[AI]）**

在 `docs/Rust学习打卡记录.md` 末尾补 2026-09-04 记录：完成审计字段补项
（迁移、实体、repo 统一注入、api/DTO、codegen 支持），注明测试数与提交。

- [x] **步骤 5：Commit（[用户]）**

```bash
git add docs/Rust学习打卡记录.md
git commit -m "docs: 2026-09-04 W5 审计字段补项打卡"
```

---

## 自检结论

- 范围明确：6 张配置主表加双字段，日志表与关联表不动，理由已在设计决策中写明。
- 注入点收敛：审计写入集中在 11 个 repo 函数，编译期即可发现遗漏。
- 破坏性评估：加列不破坏现有代码（`NotSet` 不进 SQL）；签名变更导致的报错会在
  `cargo check` 中全部暴露，按任务 4 → 5 顺序推进可逐步收敛。
- 存量与种子数据保持 NULL，不做无依据的回填。
- codegen 改动有独立单测保护，不影响已生成的字典域实体。
