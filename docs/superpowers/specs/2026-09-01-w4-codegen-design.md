# W4 代码生成器设计

> 日期：2026-09-01 ｜ 状态：待用户审查
> 依据：Rust 学习计划 §W4「代码生成器（学完立刻用）」

## 1. 目标

从「域定义 JSON」自动生成一个业务域的完整后端切片：

- `src/entity/sys_<table>.rs`（SeaORM Entity，仿 sea-orm-cli 输出）
- `src/modules/<domain>/{api,service,repo,dto}.rs` + `mod.rs`

生成物必须直接编译、遵循项目既有约定（命名 / 路由顺序 / 通用分页），
并立即用于产出 W5/W6 模块骨架。第一版验证目标：**数据字典（dict）域**。

## 2. 架构

- 独立 binary crate：`codegen/`（与 `migrations/` 并列）
- 输入：`codegen/defs/<domain>.json` 域定义文件
- 输出：写入 `src/entity/` 与 `src/modules/<domain>/`
- 模板：Rust 模板字符串 + `format!`，**不引入**外部模板引擎（学习点落在类型映射与字符串生成）
- 类型映射：定义文件中的 `rust_type` 直接映射 SeaORM `ColumnType`

## 3. 域定义格式（JSON）

```json
{
  "domain": "dict",
  "table": "sys_dict",
  "comment": "数据字典",
  "fields": [
    { "name": "id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "primary": true, "auto_increment": true },
    { "name": "type_code", "rust_type": "String", "sql_type": "VARCHAR(64)", "optional": false, "unique": true, "comment": "字典类型编码" },
    { "name": "label", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": false, "comment": "字典项标签" },
    { "name": "value", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": false, "comment": "字典项值" },
    { "name": "sort", "rust_type": "i32", "sql_type": "INT", "optional": true, "default": 0, "comment": "排序" },
    { "name": "status", "rust_type": "i8", "sql_type": "TINYINT", "optional": true, "default": 1, "comment": "状态" },
    { "name": "remark", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": true, "comment": "备注" },
    { "name": "created_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "updated_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "deleted_at", "rust_type": "Option<DateTime>", "sql_type": "DATETIME", "readonly": true, "soft_delete": true }
  ],
  "unique_fields": ["type_code"],
  "filters": [
    { "field": "type_code", "kind": "exact" },
    { "field": "label", "kind": "keyword" },
    { "field": "status", "kind": "exact" }
  ]
}
```

字段属性语义：

| 属性 | 含义 |
|---|---|
| `primary` / `auto_increment` | 主键；Create 不含、Update 含 `id` |
| `readonly` | `created_at` / `updated_at` / `deleted_at` 等由 DB/系统维护，Create/Update 不含 |
| `soft_delete` | 标记软删字段；repo 默认过滤、删除时置值 |
| `unique` | 唯一约束；生成 `find_by_<field>_include_deleted` 查重辅助与 service 查重逻辑 |
| `filters` | 分页过滤声明：`exact` 精确 / `keyword` 模糊，生成 `<Domain>Filter` |

## 4. 生成内容

### 4.1 entity

仿 sea-orm-cli 模板：`DeriveEntityModel` + 字段注释 + `DeriveRelation` + `ActiveModelBehavior`。
`deleted_at` 生成 `Option<DateTime>`。

### 4.2 repo

- `find_by_id`（排除软删）
- `find_page(db, &<Domain>Filter, page_index, page_size)`（复用 `crate::utils::paginate`）
- `create` / `update`（ActiveModel 入参）
- `soft_delete`（事务：软删主表，返回 `bool`）
- `find_by_<unique_field>_include_deleted`（每个 `unique` 字段各一个）

### 4.3 service

- `page_<domain>`：dto → `<Domain>Filter` 构造
- `create_<domain>`：唯一字段查重（含软删）→ 构造 ActiveModel（默认值）→ repo
- `update_<domain>`：`find_by_id` 判存在 → 唯一字段查重排除自身 → 全量覆盖 → repo
- `get_<domain>` / `delete_<domain>`：判存在 + Biz
- 业务错误消息统一中文（`<实体>不存在`、`<字段>已存在`）

### 4.4 dto

- `<Domain>Resp`（`From<Model>`，排除 readonly 外的字段）
- `<Domain>ListReq`（`#[serde(flatten)] PageQuery` + filters）
- `<Domain>Filter`（repo 入参，过滤字段与分页分离）
- `Create<Domain>Req`（非 readonly 字段，`optional` 为 `Option<T>`）
- `Update<Domain>Req`（全量必填，含 `id`）
- `<Domain>IdReq`

### 4.5 api + mod

- 五端点 `list/create/update/get/delete`（`POST + JsonBody`，`ApiResult<T>`）
- `mod.rs`：模块声明 + `routes()`（顺序 `list → create → update → get → delete`）

## 5. 类型映射表

| `rust_type` | Rust 类型 | 生成默认值表达式 |
|---|---|---|
| `u64` | `u64` | `0` |
| `i64` | `i64` | `0` |
| `i32` | `i32` | `0` |
| `i8` | `i8` | `1`（status 类） |
| `bool` | `bool` | `false` |
| `String` | `String` | `String::new()` |
| `Text` | `String` | `String::new()` |
| `DateTime` | `chrono::NaiveDateTime` | （只读，不生成默认） |

## 6. CLI

```
cd codegen && cargo run -- generate ../codegen/defs/dict.json
```

执行后输出生成文件清单，并在 `src/modules/<domain>/mod.rs` 生成路由；
`router.rs` 挂载由人工一行追加（生成器打印提示，不自动改全局路由文件）。

## 7. 测试策略

- 生成器单元测试：定义解析（serde）、必填校验、类型映射、模板输出包含关键模式
  （entity derive / `paginate` / `soft_delete` / 路由顺序）
- 生成物验证：对 dict 定义生成代码 → 项目编译 + `cargo test` 全绿
- 生成的 repo / service 测试由生成器一并输出（与既有域测试模式一致），验证 CRUD 真实可用

## 8. 验收标准

1. `codegen` 能从 dict 定义生成 6 个文件 + 路由
2. 生成代码通过 `cargo fmt --check` 与 `cargo check`
3. 生成的 dict 域测试全绿（分页 / 唯一查重 / 软删除 / CRUD）
4. 全量 `cargo test` 保持绿

## 9. 范围外（后续迭代）

- 多对多关联维护（如 `sys_role_menu` 模式）生成
- 前端 TS 类型 / API 客户端生成
- 菜单 + 权限码自动注册
- 生成器的幂等覆盖/增量更新（第一版仅支持新域生成）
