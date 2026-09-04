# 数据字典模块（W5-3）实现计划

> **面向 AI 代理的工作者：** 本计划遵循项目协作约定（W3 起固定分工）：
> AI 编写失败测试与脚手架、做最终 review 与验证；用户手动实现业务代码。
> 步骤中用 **[AI]** / **[用户]** 标注执行者，复选框跟踪进度。

**目标：** 把 W4 期 codegen 验证用的 `sys_dict` 单表，改造为 gin-vue-admin
的两级数据字典：`sys_dictionary`（类型）+ `sys_dictionary_detail`（字典项），
含数据搬迁、级联软删、前端下拉端点 `get-by-type`、种子菜单与 6 个权限码。

**架构：** 单个 `dictionary` 域，域内两套 repo / service / api 函数；
两个路由组 `dictionary` 与 `dictionary-detail` 分别挂载；
codegen 只用于生成两个 entity，四件套手写（生成器不支持一对多）。

**技术栈：** Salvo 0.95.2、SeaORM 1.x、serde、chrono；codegen 生成器产出 entity。

**规格依据：** `docs/superpowers/specs/2026-09-03-w5-dictionary-design.md`

> **收尾补记（2026-09-03）**：模块已全部落地并提交，逐条 Commit 实际被合并执行——
> `397dde4`（迁移+搬迁+退役）、`55d4ec4`（域实现+测试+退役旧域）、`17a66cf`（种子）、
> `5173135`（打卡文档）；故下方 Commit 步骤按实际提交勾销。
> 其余 [用户] 红/绿验证步骤以最终全量主项目 133/133 + codegen 14/14 全绿为准同步勾选。
> 唯一未勾选项：任务 9 步骤 3 手动冒烟（可选，未执行）。

---

## 文件结构

创建：
- `migrations/src/m20260903_000009_create_sys_dictionary.rs`（建表 + 搬迁 + drop 旧表）
- `codegen/defs/dictionary.json`、`codegen/defs/dictionary_detail.json`（域定义档案）
- `src/entity/sys_dictionary.rs`、`src/entity/sys_dictionary_detail.rs`（codegen 生成）
- `src/modules/dictionary/{mod,api,service,repo,dto}.rs`（手写）

删除：
- `src/modules/dict/`（整个目录）
- `src/entity/sys_dict.rs`
- `codegen/defs/dict.json`

修改：
- `migrations/src/lib.rs`：注册迁移
- `src/entity/mod.rs`、`src/entity/prelude.rs`：换实体声明
- `src/modules/mod.rs`：`dict` → `dictionary`
- `src/infra/router.rs`：替换 dict 路由块为两个新路由组
- `src/infra/seed.rs`：MENU_SEEDS 追加数据字典菜单与 6 个按钮权限码
- `docs/Rust学习打卡记录.md`：补充 2026-09-03 记录

测试（写入各实现文件 `#[cfg(test)]`，由 AI 完成）：
- `src/modules/dictionary/repo.rs`：两张表的分页/过滤/软删/级联/唯一性
- `src/modules/dictionary/service.rs`：业务规则、级联、get-by-type
- `src/infra/seed.rs`：数据字典菜单与权限码存在断言

---

## 任务 1：`sys_dictionary` 迁移 + 数据搬迁（[用户]）

**文件：**
- 创建：`migrations/src/m20260903_000009_create_sys_dictionary.rs`
- 修改：`migrations/src/lib.rs`

- [x] **步骤 1：编写迁移文件**

对照 `migrations/src/m20260903_000007_create_sys_operation_log.rs` 的结构。
`up` 分三段：建两张表与索引 → 搬迁存量数据 → 删除 `sys_dict`。

```rust
//! W5 迁移：数据字典两级表（类型 + 字典项），并搬迁 sys_dict 存量数据。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysDictionary {
    Table,
    Id,
    Name,
    Type,
    Status,
    Remark,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum SysDictionaryDetail {
    Table,
    Id,
    DictionaryId,
    Label,
    Value,
    Extend,
    Sort,
    Status,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1) 字典类型表
        manager
            .create_table(
                Table::create()
                    .table(SysDictionary::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysDictionary::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Name)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Type)
                            .string_len(64)
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::Remark)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysDictionary::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysDictionary::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;

        // 2) 字典项表
        manager
            .create_table(
                Table::create()
                    .table(SysDictionaryDetail::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::DictionaryId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Label)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Value)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Extend)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Sort)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::Status)
                            .tiny_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysDictionaryDetail::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysDictionaryDetail::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_dictionary_detail_dictionary_id")
                    .table(SysDictionaryDetail::Table)
                    .col(SysDictionaryDetail::DictionaryId)
                    .col(SysDictionaryDetail::Sort)
                    .to_owned(),
            )
            .await?;

        // 3) 搬迁 sys_dict 存量数据（旧表 type_code 唯一，严格 1:1）
        let db = manager.get_connection();
        db.execute_unprepared(
            "INSERT INTO sys_dictionary (name, type, status, remark, created_at, updated_at, deleted_at)
             SELECT type_code, type_code, status, remark, created_at, updated_at, deleted_at
             FROM sys_dict",
        )
        .await?;
        db.execute_unprepared(
            "INSERT INTO sys_dictionary_detail
               (dictionary_id, label, value, extend, sort, status, created_at, updated_at, deleted_at)
             SELECT d.id, s.label, s.value, '', s.sort, s.status, s.created_at, s.updated_at, s.deleted_at
             FROM sys_dict s
             JOIN sys_dictionary d ON d.type = s.type_code",
        )
        .await?;

        // 4) 旧表退役
        manager
            .drop_table(Table::drop().table(Alias::new("sys_dict")).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysDictionaryDetail::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SysDictionary::Table).to_owned())
            .await
    }
}
```

注意：`down` 只删除两张新表，不还原 `sys_dict`（1:N 压回 1:1 会丢数据，规格 §4 已定）。

- [x] **步骤 2：注册迁移**

`migrations/src/lib.rs` 追加 `mod m20260903_000009_create_sys_dictionary;`
并在 `Migrator::migrations()` 列表末尾追加
`Box::new(m20260903_000009_create_sys_dictionary::Migration),`。

- [x] **步骤 3：备份 + 应用迁移并验证**

搬迁前先备份旧表（一次性保险）：

```bash
docker exec salvo-vben-mysql mysql -uroot -proot salvo_vben \
  -e "CREATE TABLE sys_dict_bak_20260903 AS SELECT * FROM sys_dict"
cd migrations && DATABASE_URL='mysql://root:root@localhost:3307/salvo_vben' cargo run -- up
```

验证：

```bash
docker exec salvo-vben-mysql mysql -uroot -proot salvo_vben \
  -e "SHOW TABLES LIKE 'sys_dict%'; SHOW CREATE TABLE sys_dictionary_detail\G"
docker exec salvo-vben-mysql mysql -uroot -proot salvo_vben \
  -e "SELECT (SELECT COUNT(*) FROM sys_dict_bak_20260903) AS old_rows,
             (SELECT COUNT(*) FROM sys_dictionary) AS new_types,
             (SELECT COUNT(*) FROM sys_dictionary_detail) AS new_items"
```

预期：`sys_dict` 已消失；`old_rows == new_types == new_items`（旧表 1:1 搬迁）。

- [x] **步骤 4：Commit（[用户]）** — 已由 `397dde4` 提交（信息有改写）

```bash
git add migrations/src/m20260903_000009_create_sys_dictionary.rs migrations/src/lib.rs
git commit -m "feat(migrations): 数据字典改为两级表并搬迁 sys_dict 存量数据"
```

---

## 任务 2：域定义 JSON + entity 生成（[AI]）

**文件：**
- 创建：`codegen/defs/dictionary.json`、`codegen/defs/dictionary_detail.json`
- 创建：`src/entity/sys_dictionary.rs`、`src/entity/sys_dictionary_detail.rs`
- 修改：`src/entity/mod.rs`、`src/entity/prelude.rs`

- [x] **步骤 1：创建两份域定义（[AI]）**

`dictionary.json`：

```json
{
  "domain": "dictionary",
  "table": "sys_dictionary",
  "comment": "字典类型",
  "fields": [
    { "name": "id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "primary": true, "auto_increment": true },
    { "name": "name", "rust_type": "String", "sql_type": "VARCHAR(64)", "optional": true, "default": "", "comment": "字典名称" },
    { "name": "type", "rust_type": "String", "sql_type": "VARCHAR(64)", "unique": true, "comment": "字典类型编码" },
    { "name": "status", "rust_type": "i8", "sql_type": "TINYINT", "optional": true, "default": 1, "comment": "状态" },
    { "name": "remark", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": true, "default": "", "comment": "备注" },
    { "name": "created_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "updated_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "deleted_at", "rust_type": "Option<DateTime>", "sql_type": "DATETIME", "readonly": true, "soft_delete": true }
  ],
  "unique_fields": ["type"],
  "filters": [
    { "field": "status", "kind": "exact" }
  ]
}
```

`dictionary_detail.json`：

```json
{
  "domain": "dictionary_detail",
  "table": "sys_dictionary_detail",
  "comment": "字典项",
  "fields": [
    { "name": "id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "primary": true, "auto_increment": true },
    { "name": "dictionary_id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "optional": false, "comment": "所属字典类型 ID" },
    { "name": "label", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": true, "default": "", "comment": "展示值" },
    { "name": "value", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": true, "default": "", "comment": "字典值" },
    { "name": "extend", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": true, "default": "", "comment": "扩展值" },
    { "name": "sort", "rust_type": "i32", "sql_type": "INT", "optional": true, "default": 0, "comment": "排序" },
    { "name": "status", "rust_type": "i8", "sql_type": "TINYINT", "optional": true, "default": 1, "comment": "状态" },
    { "name": "created_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "updated_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "deleted_at", "rust_type": "Option<DateTime>", "sql_type": "DATETIME", "readonly": true, "soft_delete": true }
  ],
  "unique_fields": [],
  "filters": [
    { "field": "dictionary_id", "kind": "exact" },
    { "field": "status", "kind": "exact" }
  ]
}
```

说明：`unique_fields` 对字典项留空——按规格 §3.3，`value` 唯一只在活记录中由服务层保证，
不加数据库唯一键。

- [x] **步骤 2：运行 codegen 生成 entity（[AI]）**

```bash
cd codegen && cargo run -- generate ../codegen/defs/dictionary.json
cd codegen && cargo run -- generate ../codegen/defs/dictionary_detail.json
```

生成器会同时输出 `src/modules/dictionary*/` 四件套；本轮**只保留 entity**，
四件套在任务 4/5/6 手写，把生成出来的 `modules/dictionary/` 与
`modules/dictionary_detail/` 目录删掉即可。

- [x] **步骤 3：修正 `type` 裸标识符（[AI]）**

`type` 是 Rust 关键字，`sys_dictionary.rs` 中必须写作裸标识符：

```rust
/// 字典类型编码
pub r#type: String,
```

对应 Column 变体为 `Column::Type`。若生成器输出异常，手工改成上面这行并确认
`Column::Type` 存在。

- [x] **步骤 4：注册实体（[AI]）**

`src/entity/mod.rs`：删除 `pub mod sys_dict;`，按字母序插入
`pub mod sys_dictionary;` 与 `pub mod sys_dictionary_detail;`。
`src/entity/prelude.rs`：同步替换导出行。

- [x] **步骤 5：验证编译**

```bash
cargo check
```

预期：可能报 `modules/dict` 引用了 `sys_dict`——任务 3 会清理，此处不影响。

- [x] **步骤 6：Commit（[用户]，审阅生成物后提交）** — 已由 `55d4ec4` 一并提交

```bash
git add codegen/defs/dictionary.json codegen/defs/dictionary_detail.json \
        src/entity/sys_dictionary.rs src/entity/sys_dictionary_detail.rs \
        src/entity/mod.rs src/entity/prelude.rs
git commit -m "feat(codegen): 生成字典类型与字典项实体"
```

---

## 任务 3：退役 `sys_dict` 单表域（[用户]）

**文件：** `src/modules/dict/`、`codegen/defs/dict.json`、`src/modules/mod.rs`、
`src/infra/router.rs`

- [x] **步骤 1：删除旧域**

```bash
rm -rf src/modules/dict codegen/defs/dict.json
```

- [x] **步骤 2：换模块声明**

`src/modules/mod.rs`：`pub mod dict;` → `pub mod dictionary;`

- [x] **步骤 3：换路由挂载**

`src/infra/router.rs` 把 dict 路由块：

```rust
                // 数据字典管理：POST /api/v1/dict/{list,create,update,get,delete}（codegen 生成域）
                .push(
                    Router::with_path("dict")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::dict::routes()),
                )
```

替换为：

```rust
                // 数据字典类型：POST /api/v1/dictionary/{list,create,update,get,delete,get-by-type}
                .push(
                    Router::with_path("dictionary")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::dictionary::routes()),
                )
                // 数据字典项：POST /api/v1/dictionary-detail/{list,create,update,get,delete}
                .push(
                    Router::with_path("dictionary-detail")
                        .hoop(AuthRequired)
                        .hoop(OperationLog)
                        .push(crate::modules::dictionary::detail_routes()),
                )
```

- [x] **步骤 4：验证编译**（中间态报错随任务 4~6 落地消除，最终全量全绿）

```bash
cargo check
```

预期：因 `dictionary` 模块还不存在而报错——任务 4~6 补上即消失。
（若不想留中间态，可把本任务放在任务 6 之后执行。）

- [x] **步骤 5：Commit（[用户]）** — 已由 `55d4ec4` 提交（信息有改写）

```bash
git add -A src/modules/dict codegen/defs/dict.json src/modules/mod.rs src/infra/router.rs
git commit -m "refactor(dict): 退役 sys_dict 单表域，改挂两级数据字典路由"
```

---

## 任务 4：DTO（[用户]）

**文件：** `src/modules/dictionary/dto.rs`

- [x] **步骤 1：编写 DTO**

```rust
//! 数据字典 DTO：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::utils::PageQuery;

/// 字典类型响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryResp {
    pub id: u64,
    pub name: String,
    pub r#type: String,
    pub status: i8,
    pub remark: String,
}

impl From<sys_dictionary::Model> for DictionaryResp { /* 逐字段搬运 */ }

/// 字典类型列表请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

/// 字典类型分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct DictionaryFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDictionaryReq {
    pub name: String,
    pub r#type: String,
    pub status: Option<i8>,
    pub remark: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDictionaryReq {
    pub id: u64,
    pub name: String,
    pub r#type: String,
    pub status: i8,
    pub remark: Option<String>,
}

/// 按类型取启用字典项（`get-by-type` 响应）。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryOptionResp {
    pub id: u64,
    pub name: String,
    pub r#type: String,
    pub details: Vec<DictionaryDetailOption>,
}

/// 下拉项：只带前端渲染需要的字段。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryDetailOption {
    pub id: u64,
    pub label: String,
    pub value: String,
    pub extend: String,
    pub sort: i32,
}

/// 字典项响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct DictionaryDetailResp {
    pub id: u64,
    pub dictionary_id: u64,
    pub label: String,
    pub value: String,
    pub extend: String,
    pub sort: i32,
    pub status: i8,
}

impl From<sys_dictionary_detail::Model> for DictionaryDetailResp { /* 逐字段搬运 */ }

#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryDetailListReq {
    #[serde(flatten)]
    pub page: PageQuery,
    pub dictionary_id: Option<u64>,
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

#[derive(Debug, Clone, Default)]
pub struct DictionaryDetailFilter {
    pub dictionary_id: Option<u64>,
    pub keyword: Option<String>,
    pub status: Option<i8>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDictionaryDetailReq {
    pub dictionary_id: u64,
    pub label: String,
    pub value: String,
    pub extend: Option<String>,
    pub sort: Option<i32>,
    pub status: Option<i8>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDictionaryDetailReq {
    pub id: u64,
    pub dictionary_id: u64,
    pub label: String,
    pub value: String,
    pub extend: Option<String>,
    pub sort: i32,
    pub status: i8,
}

/// 按类型编码取字典项（`get-by-type` 请求）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DictionaryTypeReq {
    pub r#type: String,
}
```

- [x] **步骤 2：Commit（[用户]）** — 已由 `55d4ec4` 一并提交

```bash
git add src/modules/dictionary/dto.rs
git commit -m "feat(dictionary): 字典两级结构 DTO"
```

---

## 任务 5：repo 层（[AI] 测试 → [用户] 实现）

**文件：** `src/modules/dictionary/repo.rs`

- [x] **步骤 1：AI 写入失败测试**

```rust
#[cfg(test)]
mod tests {
    // 公共件：test_db() 复用 sys_dict 测试写法（Config::load + Database::connect）；
    // unique(prefix) = format!("{prefix}_{}_{}", pid, SEQ)；
    // seed_dictionary(db, r#type, status, deleted_at) / seed_detail(db, dictionary_id, value, sort, status, deleted_at)
    // cleanup：先物理删 sys_dictionary_detail，再删 sys_dictionary。

    #[tokio::test]
    async fn find_dictionary_page_filters_by_keyword_and_status_excludes_deleted() {}
    // keyword 分别命中 name 与 type；status 精确；软删记录不返回。

    #[tokio::test]
    async fn find_dictionary_by_type_include_deleted_finds_soft_deleted() {}
    // 软删记录也要能查到（验证 type 唯一键含软删占位的语义）。

    #[tokio::test]
    async fn find_detail_page_filters_by_dictionary_and_keyword_excludes_deleted() {}
    // dictionary_id 精确；keyword 命中 label 与 value；软删排除。

    #[tokio::test]
    async fn find_enabled_details_only_alive_and_enabled_sorted_by_sort() {}
    // 造 3 条：status=1/sort=2、status=1/sort=1、status=0/sort=0、另 1 条软删；
    // 期望只返回两条且顺序为 sort 1 → 2。

    #[tokio::test]
    async fn soft_delete_details_by_dictionary_id_marks_all_alive_rows() {}
    // 2 条活 + 1 条已软删，返回 rows_affected == 2。

    #[tokio::test]
    async fn find_alive_detail_by_value_ignores_soft_deleted() {}
    // 软删过的同 value 不被返回（§3.3「软删后可重建」的核心支撑）。
}
```

- [x] **步骤 2：运行确认红（[用户]）**

```bash
cargo test dictionary::repo 2>&1 | tail -30
```

预期：编译失败，`dictionary` 模块 / 这些函数不存在。

- [x] **步骤 3：用户实现 repo**（55d4ec4 落地）

```rust
//! 数据字典数据访问：类型表与字典项表。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection};

use crate::entity::{sys_dictionary, sys_dictionary::Model as Dictionary};
use crate::entity::{sys_dictionary_detail, sys_dictionary_detail::Model as Detail};
use crate::modules::dictionary::dto::{DictionaryDetailFilter, DictionaryFilter};

/// 类型：按主键查有效记录（排除软删）。
pub async fn find_dictionary_by_id(
    db: &DatabaseConnection,
    id: u64,
) -> anyhow::Result<Option<Dictionary>> { /* ... */ }

/// 类型：分页 + 动态过滤（keyword 对 name / type 模糊）。
pub async fn find_dictionary_page(
    db: &DatabaseConnection,
    filter: &DictionaryFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Dictionary>> {
    let mut cond = Condition::all();
    if let Some(kw) = &filter.keyword {
        cond = cond.add(
            Condition::any()
                .add(sys_dictionary::Column::Name.like(format!("%{kw}%")))
                .add(sys_dictionary::Column::Type.like(format!("%{kw}%"))),
        );
    }
    if let Some(status) = filter.status {
        cond = cond.add(sys_dictionary::Column::Status.eq(status));
    }
    let select = sys_dictionary::Entity::find()
        .filter(cond)
        .filter(sys_dictionary::Column::DeletedAt.is_null())
        .order_by_desc(sys_dictionary::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 类型：查重辅助，type 唯一（含软删占位）。
pub async fn find_dictionary_by_type_include_deleted(
    db: &DatabaseConnection,
    r#type: &str,
) -> anyhow::Result<Option<Dictionary>> { /* 不加 deleted_at 过滤 */ }

pub async fn create_dictionary(
    db: &DatabaseConnection,
    model: sys_dictionary::ActiveModel,
) -> anyhow::Result<Dictionary> { Ok(model.insert(db).await?) }

pub async fn update_dictionary(
    db: &DatabaseConnection,
    model: sys_dictionary::ActiveModel,
) -> anyhow::Result<Dictionary> { Ok(model.update(db).await?) }

/// 类型：软删单条。
pub async fn soft_delete_dictionary(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    // 参照旧 dict 域 soft_delete_dict：先 find_by_id，再 Set deleted_at 后 update。
}

/// 字典项：按主键查有效记录。
pub async fn find_detail_by_id(
    db: &DatabaseConnection,
    id: u64,
) -> anyhow::Result<Option<Detail>> { /* ... */ }

/// 字典项：分页 + 动态过滤（dictionary_id 精确 + keyword 模糊）。
pub async fn find_detail_page(
    db: &DatabaseConnection,
    filter: &DictionaryDetailFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Detail>> {
    // 排序：sort 升序、id 升序
}

/// 字典项：取某类型下全部启用且未软删的项（`get-by-type` 用）。
pub async fn find_enabled_details(
    db: &DatabaseConnection,
    dictionary_id: u64,
) -> anyhow::Result<Vec<Detail>> {
    // filter dictionary_id + status=1 + deleted_at IS NULL；order_by_asc(sort)、order_by_asc(id)
}

/// 字典项：查重辅助——同类型同 value 的**活记录**（软删的不算）。
pub async fn find_alive_detail_by_value(
    db: &DatabaseConnection,
    dictionary_id: u64,
    value: &str,
) -> anyhow::Result<Option<Detail>> {
    // filter dictionary_id + value + deleted_at IS NULL
}

/// 字典项：级联软删某类型下的全部活记录，返回受影响行数。
pub async fn soft_delete_details_by_dictionary_id(
    db: &DatabaseConnection,
    dictionary_id: u64,
) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let res = sys_dictionary_detail::Entity::update_many()
        .col_expr(sys_dictionary_detail::Column::DeletedAt, Expr::value(Some(now)))
        .filter(sys_dictionary_detail::Column::DictionaryId.eq(dictionary_id))
        .filter(sys_dictionary_detail::Column::DeletedAt.is_null())
        .exec(db)
        .await?;
    Ok(res.rows_affected)
}

pub async fn create_detail(
    db: &DatabaseConnection,
    model: sys_dictionary_detail::ActiveModel,
) -> anyhow::Result<Detail> { Ok(model.insert(db).await?) }

pub async fn update_detail(
    db: &DatabaseConnection,
    model: sys_dictionary_detail::ActiveModel,
) -> anyhow::Result<Detail> { Ok(model.update(db).await?) }

/// 字典项：软删单条，返回是否实际删除。
pub async fn soft_delete_detail(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    // 同 soft_delete_dictionary
}
```

- [x] **步骤 4：运行测试验证绿（[用户]）**

```bash
cargo test dictionary::repo 2>&1 | tail -30
```

- [x] **步骤 5：Commit（[用户]）** — 已由 `55d4ec4` 提交（信息有改写）

```bash
git add src/modules/dictionary/repo.rs
git commit -m "feat(dictionary): repo 两级分页过滤、启用项查询与级联软删"
```

---

## 任务 6：service 层（[AI] 测试 → [用户] 实现）

**文件：** `src/modules/dictionary/service.rs`

- [x] **步骤 1：AI 写入失败测试**

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn create_dictionary_rejects_duplicate_type_including_soft_deleted() {}
    // 活的 + 软删的 type 都要拒绝（对齐旧 dict 域既有行为）。

    #[tokio::test]
    async fn update_dictionary_rejects_duplicate_type_excluding_self() {}

    #[tokio::test]
    async fn delete_dictionary_cascade_soft_deletes_its_details() {}
    // 类型下 2 条活字典项，删类型后 find_enabled_details 返回空。

    #[tokio::test]
    async fn create_detail_rejects_duplicate_value_among_alive_rows() {}
    // 同类型活记录 value 重复 → Biz；类型不存在 → Biz。

    #[tokio::test]
    async fn create_detail_allows_reusing_value_of_soft_deleted_row() {}
    // §3.3 的核心断言：软删后可重建同 value。

    #[tokio::test]
    async fn get_dictionary_by_type_returns_enabled_details_sorted() {}

    #[tokio::test]
    async fn get_dictionary_by_type_errors_when_type_missing_or_disabled() {}
    // 不存在 → Biz；status=0 → Biz。

    #[tokio::test]
    async fn get_and_delete_missing_return_biz_error() {}
}
```

- [x] **步骤 2：运行确认红（[用户]）**

```bash
cargo test dictionary::service 2>&1 | tail -30
```

- [x] **步骤 3：用户实现 service**

```rust
//! 数据字典业务：类型表与字典项表。

use sea_orm::{ActiveValue::Set, DatabaseConnection};

use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::modules::dictionary::dto::*;
use crate::modules::dictionary::repo as dict_repo;
use crate::utils::error::AppError;
use crate::utils::PageData;

pub async fn page_dictionaries(
    db: &DatabaseConnection,
    req: &DictionaryListReq,
) -> Result<PageData<sys_dictionary::Model>, AppError> {
    // 组装 DictionaryFilter 后调 repo::find_dictionary_page
}

/// 创建类型：type 查重（含软删占位）→ 落库。
pub async fn create_dictionary(
    db: &DatabaseConnection,
    req: &CreateDictionaryReq,
) -> Result<sys_dictionary::Model, AppError> {
    if let Some(existing) = dict_repo::find_dictionary_by_type_include_deleted(db, &req.r#type).await? {
        return Err(AppError::Biz(format!("字典类型编码已存在：{}", existing.r#type)));
    }
    // ...
}

pub async fn update_dictionary(
    db: &DatabaseConnection,
    req: &UpdateDictionaryReq,
) -> Result<sys_dictionary::Model, AppError> {
    // 判存在 → type 查重排除自身 → 全量覆盖
}

pub async fn get_dictionary(
    db: &DatabaseConnection,
    id: u64,
) -> Result<sys_dictionary::Model, AppError> {
    // 不存在 → Biz("字典类型不存在：{id}")
}

/// 删除类型：判存在 → 软删类型 → 级联软删其下字典项。
pub async fn delete_dictionary(db: &DatabaseConnection, id: u64) -> Result<u64, AppError> {
    // 返回级联删除的字典项数量，便于前端提示
}

/// 按类型编码取「类型 + 启用字典项」（前端下拉用）。
pub async fn get_dictionary_by_type(
    db: &DatabaseConnection,
    r#type: &str,
) -> Result<(sys_dictionary::Model, Vec<sys_dictionary_detail::Model>), AppError> {
    // 1) find_dictionary_by_type_include_deleted；
    // 2) 不存在 / deleted_at 非空 / status != 1 → Biz("字典类型不存在或已停用：{type}")；
    // 3) repo::find_enabled_details → 空数组不报错。
}

pub async fn page_dictionary_details(
    db: &DatabaseConnection,
    req: &DictionaryDetailListReq,
) -> Result<PageData<sys_dictionary_detail::Model>, AppError> { /* ... */ }

/// 创建字典项：类型存在性校验 → 活记录 value 查重 → 落库。
pub async fn create_dictionary_detail(
    db: &DatabaseConnection,
    req: &CreateDictionaryDetailReq,
) -> Result<sys_dictionary_detail::Model, AppError> {
    // 类型不存在 → Biz("字典类型不存在：{dictionary_id}")；
    // 同类型活记录 value 重复 → Biz("字典值已存在：{value}")。
}

pub async fn update_dictionary_detail(
    db: &DatabaseConnection,
    req: &UpdateDictionaryDetailReq,
) -> Result<sys_dictionary_detail::Model, AppError> {
    // 判存在 → 类型校验 → value 查重排除自身 → 全量覆盖
}

pub async fn get_dictionary_detail(
    db: &DatabaseConnection,
    id: u64,
) -> Result<sys_dictionary_detail::Model, AppError> {
    // 不存在 → Biz("字典项不存在：{id}")
}

pub async fn delete_dictionary_detail(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    // 不存在 → Biz；存在则 repo::soft_delete_detail
}
```

注意：错误文案必须与规格 §7 完全一致（测试按文案断言 Biz 变体即可，
文案本身由 review 环节核对）。

- [x] **步骤 4：运行测试验证绿（[用户]）**（repo/service 连续 4 次全绿）

```bash
cargo test dictionary 2>&1 | tail -30
```

- [x] **步骤 5：Commit（[用户]）** — 已由 `55d4ec4` 提交（信息有改写）

```bash
git add src/modules/dictionary/service.rs
git commit -m "feat(dictionary): 两级结构业务规则、级联软删与下拉查询"
```

---

## 任务 7：api / mod（[用户]）

**文件：** `src/modules/dictionary/api.rs`、`src/modules/dictionary/mod.rs`

- [x] **步骤 1：编写 api.rs**

函数顺序 = 类型组 CRUD → get-by-type → 字典项组 CRUD：

```rust
//! 数据字典 handler。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::modules::dictionary::dto::*;
use crate::modules::dictionary::service as dict_service;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

// —— 字典类型 ——
#[endpoint]
pub async fn list_dictionaries(
    depot: &mut Depot,
    body: JsonBody<DictionaryListReq>,
) -> ApiResult<PageResult<DictionaryResp>> { /* ... */ }

#[endpoint]
pub async fn create_dictionary(
    depot: &mut Depot,
    body: JsonBody<CreateDictionaryReq>,
) -> ApiResult<DictionaryResp> { /* ... */ }

#[endpoint]
pub async fn update_dictionary(
    depot: &mut Depot,
    body: JsonBody<UpdateDictionaryReq>,
) -> ApiResult<DictionaryResp> { /* ... */ }

#[endpoint]
pub async fn get_dictionary(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<DictionaryResp> { /* ... */ }

#[endpoint]
pub async fn delete_dictionary(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<u64> { /* 返回级联删除的字典项数 */ }

/// 按类型编码取启用字典项（特殊契约端点，排在类型组 CRUD 之后）。
#[endpoint]
pub async fn get_dictionary_by_type(
    depot: &mut Depot,
    body: JsonBody<DictionaryTypeReq>,
) -> ApiResult<DictionaryOptionResp> { /* ... */ }

// —— 字典项 ——
#[endpoint]
pub async fn list_dictionary_details(
    depot: &mut Depot,
    body: JsonBody<DictionaryDetailListReq>,
) -> ApiResult<PageResult<DictionaryDetailResp>> { /* ... */ }

#[endpoint]
pub async fn create_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<CreateDictionaryDetailReq>,
) -> ApiResult<DictionaryDetailResp> { /* ... */ }

#[endpoint]
pub async fn update_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<UpdateDictionaryDetailReq>,
) -> ApiResult<DictionaryDetailResp> { /* ... */ }

#[endpoint]
pub async fn get_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<DictionaryDetailResp> { /* ... */ }

#[endpoint]
pub async fn delete_dictionary_detail(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<()> { /* ... */ }
```

handler 内部按项目惯例 `AppState::from_depot(depot)?` 拿 db。

- [x] **步骤 2：编写 mod.rs（两个路由组）**

```rust
//! 数据字典域：类型表与字典项表两级结构。

use salvo::oapi::RouterExt;
use salvo::prelude::*;

pub mod api;
pub mod dto;
pub mod repo;
pub mod service;

/// 字典类型端点：`POST /api/v1/dictionary/{list,create,update,get,delete,get-by-type}`。
pub fn routes() -> Router {
    Router::new()
        .oapi_tags(["数据字典"])
        .push(Router::with_path("list").post(api::list_dictionaries))
        .push(Router::with_path("create").post(api::create_dictionary))
        .push(Router::with_path("update").post(api::update_dictionary))
        .push(Router::with_path("get").post(api::get_dictionary))
        .push(Router::with_path("delete").post(api::delete_dictionary))
        .push(Router::with_path("get-by-type").post(api::get_dictionary_by_type))
}

/// 字典项端点：`POST /api/v1/dictionary-detail/{list,create,update,get,delete}`。
pub fn detail_routes() -> Router {
    Router::new()
        .oapi_tags(["数据字典项"])
        .push(Router::with_path("list").post(api::list_dictionary_details))
        .push(Router::with_path("create").post(api::create_dictionary_detail))
        .push(Router::with_path("update").post(api::update_dictionary_detail))
        .push(Router::with_path("get").post(api::get_dictionary_detail))
        .push(Router::with_path("delete").post(api::delete_dictionary_detail))
}
```

- [x] **步骤 3：验证编译与既有测试**

```bash
cargo check
cargo test dictionary 2>&1 | tail -10
```

- [x] **步骤 4：Commit（[用户]）** — 已由 `55d4ec4` 提交（信息有改写）

```bash
git add src/modules/dictionary/api.rs src/modules/dictionary/mod.rs
git commit -m "feat(dictionary): 挂载两级字典管理端点与下拉契约"
```

---

## 任务 8：菜单与权限码种子（[AI] 测试 → [用户] 实现）

**文件：** `src/infra/seed.rs`

- [x] **步骤 1：AI 追加失败测试**

```rust
#[tokio::test]
async fn ensure_seed_creates_dictionary_menu_and_buttons() {
    // ensure_seed 后按 name 查 SystemDictionary（menu_type 2）与 6 个子按钮
    // （permission 分别为 system:dictionary:{create,update,delete}
    //  与 system:dictionary-detail:{create,update,delete}），都存在且未软删。
}
```

- [x] **步骤 2：运行确认红（[用户]）**（红→绿已由步骤 3/4 闭环）

```bash
cargo test infra::seed 2>&1 | tail -30
```

- [x] **步骤 3：用户追加 MENU_SEEDS**（review 时发现缺失，AI 按规格 §8 补齐并验证绿）

在 `MENU_SEEDS` 末尾（登录日志按钮之后）追加：

```rust
    // 数据字典页面 + 类型/字典项各三个按钮权限码（W5-3）
    MenuSeed {
        name: "SystemDictionary",
        title: "数据字典",
        path: "/system/dictionary",
        component: "#/views/system/dictionary/index.vue",
        icon: "lucide:book-marked",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 7,
    },
    MenuSeed {
        name: "SystemDictionaryCreate",
        title: "字典类型新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary:create",
        parent: Some("SystemDictionary"),
        sort: 1,
    },
    MenuSeed {
        name: "SystemDictionaryUpdate",
        title: "字典类型编辑",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary:update",
        parent: Some("SystemDictionary"),
        sort: 2,
    },
    MenuSeed {
        name: "SystemDictionaryDelete",
        title: "字典类型删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary:delete",
        parent: Some("SystemDictionary"),
        sort: 3,
    },
    MenuSeed {
        name: "SystemDictionaryDetailCreate",
        title: "字典项新增",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary-detail:create",
        parent: Some("SystemDictionary"),
        sort: 4,
    },
    MenuSeed {
        name: "SystemDictionaryDetailUpdate",
        title: "字典项编辑",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary-detail:update",
        parent: Some("SystemDictionary"),
        sort: 5,
    },
    MenuSeed {
        name: "SystemDictionaryDetailDelete",
        title: "字典项删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:dictionary-detail:delete",
        parent: Some("SystemDictionary"),
        sort: 6,
    },
```

- [x] **步骤 4：运行测试验证绿（[用户]）**

```bash
cargo test infra::seed 2>&1 | tail -30
```

- [x] **步骤 5：Commit（[用户]）** — 已由 `17a66cf` 提交（信息有改写）

```bash
git add src/infra/seed.rs
git commit -m "feat(dictionary): 种子菜单与类型/字典项权限码"
```

---

## 任务 9：全量验证 + Review + 打卡（[AI]）

- [x] **步骤 1：运行全量验证**

```bash
cargo fmt --check
cargo check
cargo test 2>&1 | tail -20
cd codegen && cargo test 2>&1 | tail -5
```

预期：双项目全绿；格式无 diff。

- [x] **步骤 2：AI Review**（已执行；修复项在打卡记录留档：update 查重排除自身、delete 级联与返回类型、get-by-type 状态校验、value 查重、种子补齐）

按规格逐节核对：表结构与索引、搬迁后行数一致、`sys_dict` 已退役、
两套端点顺序（list → create → update → get → delete，get-by-type 在 CRUD 后）、
级联软删、唯一性口径（type 含软删 / value 仅活记录）、`get-by-type`
只返回启用项且排序正确、停用类型返回 Biz、路由双组挂载、种子 6 个权限码、
错误文案、命名一致性（`sys_dictionary` / `dictionary` / `dictionary-detail`）。
发现的问题以「必须修复 / 建议修改」分级反馈，由 [用户] 修正后回到步骤 1 重跑。

- [ ] **步骤 3：手动冒烟（可选，[用户]）**

启动服务后：登录 → 建一个类型 `gender` → 加两个字典项 → 调
`dictionary/get-by-type` 拿到有序列表 → 删类型 → 确认字典项一并消失。

- [x] **步骤 4：更新打卡记录（[AI]）**

在 `docs/Rust学习打卡记录.md` 末尾补 2026-09-03 记录：完成 W5-3 数据字典
（两级表迁移与搬迁、级联软删、下拉端点、种子），注明测试数与提交。

- [x] **步骤 5：Commit（[用户]）** — 已由 `5173135` 提交

```bash
git add docs/Rust学习打卡记录.md
git commit -m "docs: 2026-09-03 W5-3 数据字典打卡"
```

---

## 自检结论

- 规格覆盖：迁移与搬迁（任务 1）、表结构与索引（任务 1）、退役旧域（任务 3）、
  两套接口契约（任务 4/6/7）、级联软删（任务 5/6）、下拉端点（任务 6/7）、
  唯一性口径（任务 5 测试 + 任务 6 实现）、种子（任务 8）、错误文案（任务 6/9）、
  测试策略（任务 5/6/8）。
- 无占位符：迁移给出完整文件骨架；repo / service / api / DTO 给出完整签名与关键实现；
  测试由 AI 在步骤中落盘。
- 类型一致：实体与表 `sys_dictionary` / `sys_dictionary_detail`；域 `dictionary`；
  URL `dictionary` 与 `dictionary-detail`；repo / service / api 三层函数名相互对应。
- 破坏性变更已标注：任务 1 会 `drop table sys_dict`，执行前建议按步骤 3 先备份。
