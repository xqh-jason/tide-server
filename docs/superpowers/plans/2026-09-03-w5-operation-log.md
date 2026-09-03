# 操作日志模块（W5-1）实现计划

> **面向 AI 代理的工作者：** 本计划遵循项目协作约定（W3 起固定分工）：
> AI 编写失败测试与脚手架、做最终 review 与验证；用户手动实现业务代码。
> 因此不采用 subagent-driven-development，也不由 AI 实现业务逻辑；
> 步骤中用 **[AI]** / **[用户]** 标注执行者，复选框跟踪进度。

**目标：** 新增操作日志模块：中间件自动记录全部已登录业务请求，
管理端提供只读分页 / 详情 / 单删 / 批量删除（软删），并对齐 gin-vue-admin
的 sys_operation_records 语义（表名按本项目单数规范为 sys_operation_log）。

**架构：** `OperationLog` 中间件挂在 AuthRequired 之后的所有受保护路由上；
请求体由既有 `JsonBody` 提取器读取时顺带存入 Depot（`CapturedBody`），
中间件在 `ctrl.call_next` 结束后从 Depot 取请求体、从 Response 取响应体，
脱敏 + 截断后 await 落库；管理端接口按项目垂直切片四件套提供 list / get /
delete / delete-batch。

**技术栈：** Salvo 0.95.2、SeaORM 1.x、serde_json、chrono；
codegen 生成器产出 entity + 四件套骨架。

**规格依据：** `docs/superpowers/specs/2026-09-03-w5-operation-log-design.md`
（commit d47aaef）

---

## 文件结构

创建：
- `migrations/src/m20260903_000007_create_sys_operation_log.rs`（迁移）
- `codegen/defs/operation_log.json`（域定义）
- `src/entity/sys_operation_log.rs`（codegen 生成，手工补 TEXT 注解）
- `src/modules/operation_log/{mod,api,service,repo,dto}.rs`（codegen 生成后裁剪）
- `src/middleware/op_log.rs`（OperationLog 中间件 + 脱敏/截断工具 + 集成测试）

修改：
- `migrations/src/lib.rs`：注册迁移
- `src/entity/mod.rs`：`pub mod sys_operation_log;`
- `src/modules/mod.rs`：`pub mod operation_log;`
- `src/infra/router.rs`：挂载 operation-log 路由、受保护路由追加 OperationLog
- `src/modules/auth/mod.rs`：logout 子路由追加 OperationLog
- `src/middleware/mod.rs`：`pub mod op_log;`
- `src/utils/request.rs`：新增 `CapturedBody`，JsonBody 提取时写入 Depot
- `src/infra/seed.rs`：MENU_SEEDS 追加操作日志菜单与删除按钮权限码
- `docs/Rust学习打卡记录.md`：补充 2026-09-03 记录

测试（写入各实现文件 `#[cfg(test)]`，由 AI 完成）：
- `src/modules/operation_log/repo.rs`：分页过滤/排序/软删排除/批量软删
- `src/modules/operation_log/service.rs`：page/get/delete/delete-batch 业务规则
- `src/middleware/op_log.rs`：脱敏截断单测 + 中间件自动落库集成测试
- `src/infra/seed.rs`：操作日志菜单与权限码存在断言

---

## 任务 1：sys_operation_log 迁移（[用户]）

**文件：**
- 创建：`migrations/src/m20260903_000007_create_sys_operation_log.rs`
- 修改：`migrations/src/lib.rs`

- [ ] **步骤 1：编写迁移文件**

对照现有 `migrations/src/m20260901_000005_create_sys_dict.rs` 的结构，
创建下表（完整骨架，字段语义以规格第 3 节为准）：

```rust
//! W5 迁移：sys_operation_log 操作日志表（中间件自动落库）。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysOperationLog {
    Table,
    Id,
    UserId,
    Ip,
    Method,
    Path,
    Status,
    Latency,
    Agent,
    Body,
    Resp,
    ErrorMessage,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SysOperationLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysOperationLog::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SysOperationLog::UserId).big_unsigned().not_null())
                    .col(
                        ColumnDef::new(SysOperationLog::Ip)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Method)
                            .string_len(16)
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(SysOperationLog::Path).string_len(255).not_null())
                    .col(
                        ColumnDef::new(SysOperationLog::Status)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Latency)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::Agent)
                            .string_len(255)
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(SysOperationLog::Body).text().not_null())
                    .col(ColumnDef::new(SysOperationLog::Resp).text().not_null())
                    .col(
                        ColumnDef::new(SysOperationLog::ErrorMessage)
                            .string_len(500)
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SysOperationLog::UpdatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .extra("ON UPDATE CURRENT_TIMESTAMP"),
                    )
                    .col(ColumnDef::new(SysOperationLog::DeletedAt).date_time().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_operation_log_created_at")
                    .table(SysOperationLog::Table)
                    .col(SysOperationLog::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_operation_log_user_id")
                    .table(SysOperationLog::Table)
                    .col(SysOperationLog::UserId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SysOperationLog::Table).to_owned())
            .await
    }
}
```

- [ ] **步骤 2：注册迁移**

在 `migrations/src/lib.rs` 中追加：

```rust
mod m20260903_000007_create_sys_operation_log;
```

并在 `Migrator::new` 列表末尾追加：

```rust
Box::new(m20260903_000007_create_sys_operation_log::Migration),
```

- [ ] **步骤 3：应用迁移并验证**

运行：

```bash
cd migrations && DATABASE_URL='mysql://root:root@localhost:3307/salvo_vben' cargo run -- up
```

验证表结构：

```bash
docker exec salvo-vben-mysql mysql -uroot -proot salvo_vben -e "SHOW CREATE TABLE sys_operation_log\G"
```

预期：表存在，字段与上述定义一致，含两个索引。

- [ ] **步骤 4：Commit（[用户]）**

```bash
git add migrations/src/m20260903_000007_create_sys_operation_log.rs migrations/src/lib.rs
git commit -m "feat(migrations): 新增 sys_operation_log 操作日志表"
```

---

## 任务 2：域定义 JSON + codegen 骨架（[AI]）

**文件：**
- 创建：`codegen/defs/operation_log.json`
- 创建：`src/entity/sys_operation_log.rs`、`src/modules/operation_log/*.rs`（生成器输出）
- 修改：`src/entity/mod.rs`、`src/modules/mod.rs`

- [ ] **步骤 1：创建域定义 JSON（[AI]）**

```json
{
  "domain": "operation_log",
  "table": "sys_operation_log",
  "comment": "操作日志",
  "fields": [
    { "name": "id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "primary": true, "auto_increment": true },
    { "name": "user_id", "rust_type": "u64", "sql_type": "BIGINT UNSIGNED", "optional": false, "comment": "操作人 ID" },
    { "name": "ip", "rust_type": "String", "sql_type": "VARCHAR(64)", "optional": true, "default": "", "comment": "来源 IP" },
    { "name": "method", "rust_type": "String", "sql_type": "VARCHAR(16)", "optional": true, "default": "", "comment": "请求方法" },
    { "name": "path", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": false, "comment": "请求路径" },
    { "name": "status", "rust_type": "i32", "sql_type": "INT", "optional": true, "default": 0, "comment": "HTTP 状态码" },
    { "name": "latency", "rust_type": "i64", "sql_type": "BIGINT", "optional": true, "default": 0, "comment": "请求耗时（毫秒）" },
    { "name": "agent", "rust_type": "String", "sql_type": "VARCHAR(255)", "optional": true, "default": "", "comment": "User-Agent" },
    { "name": "body", "rust_type": "String", "sql_type": "TEXT", "optional": true, "default": "", "comment": "请求体（脱敏截断后）" },
    { "name": "resp", "rust_type": "String", "sql_type": "TEXT", "optional": true, "default": "", "comment": "响应体（脱敏截断后）" },
    { "name": "error_message", "rust_type": "String", "sql_type": "VARCHAR(500)", "optional": true, "default": "", "comment": "失败提示" },
    { "name": "created_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "updated_at", "rust_type": "DateTime", "sql_type": "DATETIME", "readonly": true },
    { "name": "deleted_at", "rust_type": "Option<DateTime>", "sql_type": "DATETIME", "readonly": true, "soft_delete": true }
  ],
  "unique_fields": [],
  "filters": [
    { "field": "user_id", "kind": "exact" },
    { "field": "status", "kind": "exact" }
  ]
}
```

说明：path 模糊搜索不放在域定义里（codegen 生成后会规范为 `keyword`，
沿用 sys_api / menu 的 keyword 惯例），在任务 3/4 手工加入 DTO 与 repo 过滤。

- [ ] **步骤 2：运行 codegen（[AI]）**

```bash
cd codegen && cargo run -- generate ../codegen/defs/operation_log.json
```

预期输出 6 个文件：entity/sys_operation_log.rs 与 modules/operation_log/ 下
api / service / repo / dto / mod.rs。

- [ ] **步骤 3：实体补 TEXT 注解（[AI]）**

在生成的 `src/entity/sys_operation_log.rs` 的 body / resp 字段前补：

```rust
    /// 请求体（脱敏截断后）
    #[sea_orm(column_type = "Text")]
    pub body: String,
    /// 响应体（脱敏截断后）
    #[sea_orm(column_type = "Text")]
    pub resp: String,
```

- [ ] **步骤 4：注册模块（[AI]）**

`src/entity/mod.rs` 追加 `pub mod sys_operation_log;`
`src/modules/mod.rs` 追加 `pub mod operation_log;`

- [ ] **步骤 5：验证编译**

```bash
cargo check
```

预期：无错误（此时未挂路由，代码只是编译进 crate）。

- [ ] **步骤 6：Commit（[用户]，审阅生成物后提交）**

```bash
git add codegen/defs/operation_log.json src/entity/sys_operation_log.rs src/modules/operation_log src/entity/mod.rs src/modules/mod.rs
git commit -m "feat(codegen): 用生成器产出操作日志域骨架"
```

---

## 任务 3：repo 层（[AI] 测试 → [用户] 实现）

**文件：** `src/modules/operation_log/repo.rs`

裁剪目标：删除 create/update 之外的生成模板不动；保留 `create_operation_log`
（中间件落库用），`find_page` 增加 created_at 倒序，新增批量软删；测试段整体
替换为下方 AI 测试。

- [ ] **步骤 1：AI 写入失败测试**

删除生成模板测试段，替换为以下用例（完整代码 AI 落盘，核心断言如下）：

```rust
#[cfg(test)]
mod tests {
    // seed：直接 Entity insert，path 使用唯一前缀 op_log_<pid>_<seq>
    // 场景 1：普通记录 + user_id 命中记录 + 软删记录 + status 命中记录
    #[tokio::test]
    async fn find_page_filters_and_sorts_desc_excludes_deleted() {
        // keyword 命中 path；user_id / status 精确；软删记录不返回；
        // items 按 created_at 倒序；测后 cleanup 物理删除这些 id。
    }

    #[tokio::test]
    async fn soft_delete_batch_only_marks_alive_rows() {
        // 两条活记录 + 一条已软删：传 3 个 id，rows_affected == 2；
        // 再查全部不可见（排除软删）。
    }
}
```

- [ ] **步骤 2：运行确认红（[用户]）**

```bash
cargo test operation_log::repo 2>&1 | tail -30
```

预期：编译失败，报 `soft_delete_batch` 不存在 / 排序断言失败。

- [ ] **步骤 3：用户实现 repo**

先小步修改 dto：在 `OperationLogListReq` / `OperationLogFilter` 中增加
`keyword: Option<String>`（保留 user_id / status），删除生成的 path 过滤字段；
repo 与测试均使用 `keyword`（保持与 sys_api 的 keyword 语义一致）。

保留/新增如下骨架（对照生成文件做增删）：

```rust
/// 分页 + 动态过滤查询（keyword 匹配 path，user_id/status 精确，
/// 默认 created_at 倒序，排除软删）。
pub async fn find_page(
    db: &DatabaseConnection,
    filter: &OperationLogFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(kw) = &filter.keyword {
        cond = cond.add(sys_operation_log::Column::Path.like(format!("%{kw}%")));
    }
    if let Some(user_id) = filter.user_id {
        cond = cond.add(sys_operation_log::Column::UserId.eq(user_id));
    }
    if let Some(status) = filter.status {
        cond = cond.add(sys_operation_log::Column::Status.eq(status));
    }
    let select = sys_operation_log::Entity::find()
        .filter(cond)
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .order_by_desc(sys_operation_log::Column::CreatedAt);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建日志记录（中间件落库入口，无业务校验）。
pub async fn create_operation_log(
    db: &DatabaseConnection,
    model: sys_operation_log::ActiveModel,
) -> anyhow::Result<Model> {
    Ok(model.insert(db).await?)
}

/// 软删单条：返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_operation_log(
    db: &DatabaseConnection,
    id: u64,
) -> anyhow::Result<bool> {
    // 参照 dict 域 soft_delete_dict：先 find_by_id，再 Set deleted_at 后 update。
}

/// 批量软删：只处理存在且未删除的 id，返回受影响行数。
pub async fn soft_delete_batch(
    db: &DatabaseConnection,
    ids: &[u64],
) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let res = sys_operation_log::Entity::update_many()
        .col_expr(
            sys_operation_log::Column::DeletedAt,
            Expr::value(Some(now)),
        )
        .filter(sys_operation_log::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_operation_log::Column::DeletedAt.is_null())
        .exec(db)
        .await?;
    Ok(res.rows_affected)
}
```

注意：删除生成模板中的 `update_operation_log`；`create_operation_log` 保留；
文件头注释改为「操作日志数据访问」。

- [ ] **步骤 4：运行测试验证绿（[用户]）**

```bash
cargo test operation_log::repo 2>&1 | tail -30
```

预期：repo 相关测试全部 PASS。

- [ ] **步骤 5：Commit（[用户]）**

```bash
git add src/modules/operation_log/repo.rs
git commit -m "feat(operation-log): repo 分页过滤、倒序与批量软删"
```

---

## 任务 4：DTO 与 service 层（[AI] 测试 → [用户] 实现）

**文件：** `src/modules/operation_log/dto.rs`、`src/modules/operation_log/service.rs`

- [ ] **步骤 1：AI 写入失败测试**

替换 service.rs 生成测试段，覆盖：

```rust
// page_operation_logs：keyword / user_id / status 过滤透传（seed 直接 repo insert）
#[tokio::test]
async fn page_operation_logs_filters_by_keyword_user_and_status() {}

// get / delete 不存在记录返回 AppError::Biz("操作日志不存在：{id}")
#[tokio::test]
async fn get_and_delete_missing_return_biz_error() {}

// delete-batch 空数组返回 0 且不执行 SQL；混合 id 忽略不存在只删活的
#[tokio::test]
async fn delete_batch_skips_missing_and_empty_ok() {}
```

- [ ] **步骤 2：运行确认红（[用户]）**

```bash
cargo test operation_log::service 2>&1 | tail -30
```

预期：编译失败（DTO 类型名 / service 函数缺失）。

- [ ] **步骤 3：用户裁剪 dto.rs**

删除生成模板的 `CreateOperationLogReq` / `UpdateOperationLogReq`，
保留 `OperationLogFilter` 并改为三个过滤字段，新增两个响应结构与批量删除请求：

```rust
/// 列表项响应：不含 body / resp。
#[derive(Debug, Serialize, ToSchema)]
pub struct OperationLogItem {
    pub id: u64,
    pub user_id: u64,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency: i64,
    pub agent: String,
    pub error_message: String,
    pub created_at: String,
}

/// 详情响应：含脱敏截断后的 body / resp。
#[derive(Debug, Serialize, ToSchema)]
pub struct OperationLogDetail {
    pub id: u64,
    pub user_id: u64,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency: i64,
    pub agent: String,
    pub error_message: String,
    pub created_at: String,
    pub body: String,
    pub resp: String,
}

impl From<sys_operation_log::Model> for OperationLogItem {
    // created_at 用 format!("{}", m.created_at) 转字符串
}

impl From<sys_operation_log::Model> for OperationLogDetail {
    // 同上并追加 body / resp
}

/// 批量删除请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteBatchReq {
    pub ids: Vec<u64>,
}
```

`OperationLogListReq` / `OperationLogFilter` 的过滤字段为
`keyword: Option<String>`（path 模糊）、`user_id: Option<u64>`、
`status: Option<i32>`，分页内嵌 `PageQuery` 保持生成样式。

- [ ] **步骤 4：用户实现 service**

```rust
pub async fn page_operation_logs(
    db: &DatabaseConnection,
    req: &OperationLogListReq,
) -> Result<PageData<sys_operation_log::Model>, AppError> {
    // 与 dict page_dicts 同构：组装 OperationLogFilter 后调 repo::find_page
}

pub async fn get_operation_log(
    db: &DatabaseConnection,
    id: u64,
) -> Result<sys_operation_log::Model, AppError> {
    // 不存在返回 Biz("操作日志不存在：{id}")
}

pub async fn delete_operation_log(
    db: &DatabaseConnection,
    id: u64,
) -> Result<(), AppError> {
    // 不存在返回 Biz；存在则 repo::soft_delete_operation_log
}

/// 批量软删：空数组返回 0；repo 层忽略不存在的 id。
pub async fn delete_operation_log_batch(
    db: &DatabaseConnection,
    ids: &[u64],
) -> Result<u64, AppError> {
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(repo::soft_delete_batch(db, ids).await?)
}
```

注意：删除生成的 `create_operation_log` / `update_operation_log` service 函数。

- [ ] **步骤 5：运行测试验证绿（[用户]）**

```bash
cargo test operation_log 2>&1 | tail -30
```

- [ ] **步骤 6：Commit（[用户]）**

```bash
git add src/modules/operation_log/dto.rs src/modules/operation_log/service.rs
git commit -m "feat(operation-log): 只读分页/详情与软删 service 契约"
```

---

## 任务 5：api / mod / 路由挂载（[用户]）

**文件：** `src/modules/operation_log/api.rs`、`src/modules/operation_log/mod.rs`、
`src/infra/router.rs`

- [ ] **步骤 1：裁剪 api.rs**

删除生成的 create/update handler，保留 list/get/delete，新增 delete-batch：

```rust
pub async fn list_operation_logs(
    depot: &mut Depot,
    body: JsonBody<OperationLogListReq>,
) -> ApiResult<PageResult<OperationLogItem>> {
    // 参照 dict api::list_dicts：service page 后 .into()
}

pub async fn get_operation_log(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<OperationLogDetail> {
    // service get 后 .into()
}

pub async fn delete_operation_log(
    depot: &mut Depot,
    body: JsonBody<IdReq>,
) -> ApiResult<()> {
    // service delete
}

/// 批量删除（特殊契约端点，排在 delete 后）。
pub async fn delete_operation_log_batch(
    depot: &mut Depot,
    body: JsonBody<DeleteBatchReq>,
) -> ApiResult<u64> {
    // service delete_operation_log_batch(&state.db, &req.ids)
}
```

handler 内部按项目惯例 `AppState::from_depot(depot)?` 拿 db。

- [ ] **步骤 2：裁剪 mod.rs**

只保留 list → get → delete → delete-batch，并把生成路径改为连字符
（与契约 URL 一致）：

```rust
pub fn routes() -> Router {
    Router::with_path("operation-log")
        .oapi_tags(["操作日志"])
        .push(Router::with_path("list").post(api::list_operation_logs))
        .push(Router::with_path("get").post(api::get_operation_log))
        .push(Router::with_path("delete").post(api::delete_operation_log))
        .push(Router::with_path("delete-batch").post(api::delete_operation_log_batch))
}
```

- [ ] **步骤 3：router.rs 挂载**

在现有 dict 路由块之后追加（AuthRequired 保护）：

```rust
.push(
    Router::with_path("operation-log")
        .hoop(AuthRequired)
        .push(crate::modules::operation_log::routes()),
)
```

- [ ] **步骤 4：验证编译与既有测试**

```bash
cargo check
cargo test operation_log 2>&1 | tail -10
```

预期：编译通过、operation_log 测试全绿。

- [ ] **步骤 5：Commit（[用户]）**

```bash
git add src/modules/operation_log/api.rs src/modules/operation_log/mod.rs src/infra/router.rs
git commit -m "feat(operation-log): 挂载只读管理端点"
```

---

## 任务 6：请求体捕获 + OperationLog 中间件（[AI] 测试 → [用户] 实现）

**文件：**
- 修改：`src/utils/request.rs`（CapturedBody 注入）
- 创建：`src/middleware/op_log.rs`
- 修改：`src/middleware/mod.rs`、`src/infra/router.rs`、`src/modules/auth/mod.rs`

- [ ] **步骤 1：AI 写入失败测试（op_log.rs 内）**

```rust
#[cfg(test)]
mod tests {
    // 1) sanitize_json_text：password/token/secret 键被替换为 ***，
    //    普通键与嵌套数组内容保留；
    // 2) truncate 超长文本落在 UTF-8 边界且尾部带 ...(截断)；
    // 3) 中间件集成测试：
    //    - 构造 Request/Depot/Response/FlowCtrl（参考 auth.rs 测试风格）；
    //    - depot 注入 AppState（真实 db）、AuthUser { user_id: 1 }、
    //      CapturedBody("{\"password\":\"x\",\"name\":\"a\"}")；
    //    - handlers = [OperationLog, EchoHandler]（EchoHandler 返回
    //      ApiResponse::ok(json!({"ok":true}))）；
    //    - call_next 后断言 sys_operation_log 出现该行：user_id=1、
    //      path 匹配、body 不含 "x" 且含 "***"；
    //    - 结束后按 id 物理清理。
}
```

- [ ] **步骤 2：运行确认红（[用户]）**

```bash
cargo test middleware::op_log 2>&1 | tail -30
```

预期：编译失败（中间件 / CapturedBody 不存在）。

- [ ] **步骤 3：用户实现 utils/request.rs 捕获**

在文件顶部类型区新增：

```rust
/// JsonBody 解析时缓存的原始请求体（供操作日志中间件取用）。
#[derive(Debug, Clone)]
pub struct CapturedBody(pub String);
```

把 `JsonBody::extract` 里的调用改为传入 depot，并让提取函数在成功读取
payload 后写入 Depot（原始 JSON 字符串，读取失败/空 body 不注入）：

```rust
async fn extract_json_body<'de, T>(
    req: &'de mut Request,
    depot: &'de mut Depot,
) -> Result<T, BodyParamError> {
    // 原 payload 读取逻辑不变；
    // 在反序列化之前：
    depot.insert_typed(CapturedBody(String::from_utf8_lossy(payload).into_owned()));
    // 继续 path_to_error 反序列化
}
```

`JsonBody::extract` 中把 `extract_json_body(req).await` 改为
`extract_json_body(req, depot).await`（depot 参数从 `_depot` 改名使用）。

- [ ] **步骤 4：用户实现 op_log.rs（核心骨架）**

```rust
//! 操作日志中间件（W5）：已登录业务请求自动落库（gin-vue-admin 对齐）。
//!
//! 挂载位置：AuthRequired 之后。请求体由 utils::request::CapturedBody 在
//! JsonBody 提取时写入 Depot；响应体在 ctrl.call_next 后从 Response 取出再放回；
//! 入库失败只记 tracing 错误，不影响业务响应。

use salvo::prelude::*;
use sea_orm::ActiveValue::Set;

use crate::entity::sys_operation_log;
use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::utils::request::CapturedBody;

/// 最大记录长度（字节）。
const MAX_BODY_BYTES: usize = 4096;
/// 敏感键：命中后值替换为 ***。
const SENSITIVE_KEYS: &[&str] = &[
    "password", "old_password", "new_password", "token", "authorization", "secret",
];

pub struct OperationLog;

#[async_trait]
impl Handler for OperationLog {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        let started = std::time::Instant::now();
        // AuthRequired 已通过，正常必有 AuthUser；取不到时给 0 兜底
        let user_id = depot
            .get_typed::<AuthUser>()
            .map(|u| u.user_id)
            .unwrap_or(0);

        ctrl.call_next(req, depot, res).await;

        // 请求体：JsonBody 提取器在 call_next 内写入 Depot
        let body = depot
            .get_typed::<CapturedBody>()
            .map(|b| b.0.clone())
            .unwrap_or_default();

        // 响应体：取出记录后必须放回，客户端才能收到
        let (resp, error_message) = capture_response(res);

        let latency_ms = started.elapsed().as_millis() as i64;
        let status = res.status_code.map(|s| s.as_u16() as i32).unwrap_or(200);

        let Some(state) = depot.get_typed::<AppState>().ok() else {
            tracing::error!("op log: app state missing");
            return;
        };

        let model = sys_operation_log::ActiveModel {
            user_id: Set(user_id),
            ip: Set(ip_text(req)),
            method: Set(req.method().as_str().to_string()),
            path: Set(req.uri().path().to_string()),
            status: Set(status),
            latency: Set(latency_ms),
            agent: Set(agent_text(req)),
            body: Set(sanitize_and_truncate(&body)),
            resp: Set(sanitize_and_truncate(&resp)),
            error_message: Set(truncate_utf8(&error_message, 500)),
            ..Default::default()
        };

        if let Err(err) = crate::modules::operation_log::repo::create_operation_log(
            &state.db,
            model,
        )
        .await
        {
            tracing::error!(%err, "operation log 落库失败");
        }
    }
}
```

工具函数骨架：

```rust
fn ip_text(req: &Request) -> String {
    req.remote_addr().ip().to_string()
}

fn agent_text(req: &Request) -> String {
    req.headers()
        .get(salvo::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .chars()
        .take(255)
        .collect()
}

/// 取出响应体文本并放回原 body；Error 变体记录到 error_message。
fn capture_response(res: &mut Response) -> (String, String) {
    let saved = res.take_body();
    let (text, error_message) = match &saved {
        ResBody::Once(bytes) => (String::from_utf8_lossy(bytes).into_owned(), String::new()),
        ResBody::Chunks(chunks) => {
            let mut text = String::new();
            for chunk in chunks {
                text.push_str(&String::from_utf8_lossy(chunk));
            }
            (text, String::new())
        }
        ResBody::Error(status_error) => (
            String::new(),
            format!("响应未完成（HTTP 错误由 catcher 统一处理）: {status_error}"),
        ),
        _ => (String::new(), String::new()),
    };
    *res.body_mut() = saved;
    (text, error_message)
}

/// 脱敏后截断：JSON 可解析时递归替换敏感键；非 JSON 原样截断。
fn sanitize_and_truncate(text: &str) -> String {
    let text = if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
        redact(&mut value);
        serde_json::to_string(&value).unwrap_or_else(|_| text.to_string())
    } else {
        text.to_string()
    };
    truncate_utf8(&text, MAX_BODY_BYTES)
}

fn redact(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if SENSITIVE_KEYS.contains(&key.as_str()) {
                    *val = serde_json::Value::String("***".to_string());
                } else {
                    redact(val);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

/// UTF-8 安全截断：超长时在字符边界截断并追加 ...(截断)。
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...(截断)", &s[..end])
}
```

注意：`ResBody::Chunks` 分支按 VecDeque<Bytes> 顺序拼接；
`sanitize_and_truncate` 对非 JSON 文本调用 `truncate_utf8(...)` 后直接返回，
不需要再拼后缀。

- [ ] **步骤 5：注册与挂载中间件（[用户]）**

`src/middleware/mod.rs` 追加 `pub mod op_log;`。

router.rs 中所有受保护业务路由（user / menu / dict / role / sys-api /
operation-log）在 `.hoop(AuthRequired)` 后追加 `.hoop(op_log::OperationLog)`；
例如：

```rust
.push(
    Router::with_path("user")
        .hoop(AuthRequired)
        .hoop(crate::middleware::op_log::OperationLog)
        .push(crate::modules::user::routes())
        .push(crate::modules::menu::user_routes()),
)
```

`src/modules/auth/mod.rs` 的 logout 子路由在 AuthRequired 后追加 OperationLog。
login / health 不加。

- [ ] **步骤 6：运行测试验证绿（[用户]）**

```bash
cargo test middleware::op_log 2>&1 | tail -30
cargo test operation_log 2>&1 | tail -10
cargo check
```

- [ ] **步骤 7：Commit（[用户]）**

```bash
git add src/utils/request.rs src/middleware/op_log.rs src/middleware/mod.rs src/infra/router.rs src/modules/auth/mod.rs
git commit -m "feat(operation-log): 请求日志中间件自动落库（脱敏截断）"
```

---

## 任务 7：菜单与权限码种子（[AI] 测试 → [用户] 实现）

**文件：** `src/infra/seed.rs`

- [ ] **步骤 1：AI 追加失败测试**

在 seed.rs 测试段新增：

```rust
#[tokio::test]
async fn ensure_seed_creates_operation_log_menu_and_button() {
    // ensure_seed 后按 name 查 SystemOperationLog（menu_type 2）与其子按钮
    // （permission = "system:operation-log:delete"），都存在且未软删。
}
```

- [ ] **步骤 2：运行确认红（[用户]）**

```bash
cargo test infra::seed 2>&1 | tail -30
```

- [ ] **步骤 3：用户追加 MENU_SEEDS**

在 `MENU_SEEDS` 的 API 管理按钮之后、数组末尾追加：

```rust
    // 操作日志页面 + 删除权限码（W5）
    MenuSeed {
        name: "SystemOperationLog",
        title: "操作日志",
        path: "/system/operation-log",
        component: "#/views/system/operation-log/index.vue",
        icon: "lucide:scroll-text",
        menu_type: 2,
        permission: "",
        parent: Some("System"),
        sort: 5,
    },
    MenuSeed {
        name: "SystemOperationLogDelete",
        title: "操作日志删除",
        path: "",
        component: "",
        icon: "",
        menu_type: 3,
        permission: "system:operation-log:delete",
        parent: Some("SystemOperationLog"),
        sort: 1,
    },
```

（现有测试用 MENU_SEEDS.len() 断言总数，会自动跟随。）

- [ ] **步骤 4：运行测试验证绿（[用户]）**

```bash
cargo test infra::seed 2>&1 | tail -30
```

- [ ] **步骤 5：Commit（[用户]）**

```bash
git add src/infra/seed.rs
git commit -m "feat(operation-log): 种子菜单与删除权限码"
```

---

## 任务 8：全量验证 + Review + 打卡（[AI]）

- [ ] **步骤 1：运行全量验证**

```bash
cargo fmt --check
cargo check
cargo test 2>&1 | tail -20
cd codegen && cargo test 2>&1 | tail -5
```

预期：双项目全绿；格式无 diff。

- [ ] **步骤 2：AI Review**

按规格逐节核对：表结构、list 不含 body/resp、详情含、delete-batch 空数组
语义、脱敏键集合与 4 KB 截断、路由与种子、软删口径、命名与顺序
（list → get → delete → delete-batch）。发现的问题以“必须修复/建议修改”
分级反馈，由 [用户] 修正后回到步骤 1 重跑。

- [ ] **步骤 3：手动冒烟（可选，[用户]）**

启动服务后调用登录 + 一个业务接口，再 `operation-log/list` 应看到记录。

- [ ] **步骤 4：更新打卡记录（[AI]）**

在 `docs/Rust学习打卡记录.md` 末尾补 2026-09-03 记录：完成 W5-1 操作日志
（表迁移 / 中间件自动落库 / 管理端只读接口 / 种子），注明测试数与提交。

- [ ] **步骤 5：Commit（[用户]）**

```bash
git add docs/Rust学习打卡记录.md
git commit -m "docs: 2026-09-03 W5-1 操作日志打卡"
```

---

## 自检结论

- 规格覆盖：迁移（任务 1）、中间件与数据流（任务 6）、四个接口与列表不含
  body/resp（任务 4/5）、脱敏截断（任务 6）、种子（任务 7）、错误降级
  （任务 6 实现 + 任务 8 review）、测试策略（任务 3/4/6/7）。
- 无占位符：所有步骤均给骨架或可执行命令；测试文件由 AI 在步骤中落盘。
- 类型一致：实体/表 sys_operation_log；域 operation_log；URL operation-log；
  DTO 名 OperationLogItem / OperationLogDetail / DeleteBatchReq；
  repo 函数名与 service/api 相互对应。
