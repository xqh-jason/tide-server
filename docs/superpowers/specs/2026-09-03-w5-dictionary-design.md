# W5-3 数据字典模块设计规格（gin-vue-admin 对齐）

> 日期：2026-09-03
> 状态：待用户 review
> 模块位置：W5 通用模块第 3 项（顺序：操作日志 → 登录日志 → 数据字典 → 文件上传 → 图形验证码 → 系统配置）

## 1. 背景与目标

W4 期用 `sys_dict` 单表验证了代码生成器。该模型把「字典类型」和「字典项」
压在同一行，且 `type_code` 被设为唯一键——**一个类型只能有一个字典项**，
这在语义上是错的。

gin-vue-admin 的正确模型是两级结构：

- `sys_dictionaries`：字典**类型**（如 `gender`、`status`）
- `sys_dictionary_details`：类型下的**字典项**（如 男/女、启用/停用）

本规格把现有 `sys_dict` 改造成上述两级结构，同时遵守本项目既有约定：

- 统一契约：所有接口 `POST + JSON body`，成功 `code = 1`，失败 `code = 0`；
- 表名单数命名；主表软删、查询默认过滤 `deleted_at IS NULL`；
- 接口与文件内函数顺序遵循垂直切片四件套约定；
- codegen 产出骨架，业务实现由用户手动完成，测试由 AI 编写并做最终 review。

## 2. 决策记录

| # | 决策点 | 结论 | 理由 |
|---|---|---|---|
| D1 | 表名单复数 | 单数 `sys_dictionary` / `sys_dictionary_detail` | 不跟 GVA 的复数 `sys_dictionaries` / `sys_dictionary_details`，否则破坏本项目 `sys_user` / `sys_menu` / `sys_api` 的单数规范 |
| D2 | `sys_dict` 存量 | 新建两张表 → 搬迁数据 → `drop table sys_dict` | W4 生成器验证资产不白丢；两级结构下 `sys_dict` 是双份真相，不保留 |
| D3 | 模块组织 | 一个 `dictionary` 域，内部分类型/详情两套函数 | 两张表耦合紧（级联删、下拉查询），拆两个域会互相引用 |
| D4 | codegen 用法 | 只用它生成两个 entity，四件套手写 | 现有生成器只吃单表域定义，表达不了一对多与级联 |
| D5 | 下拉端点 | 新增 `get-by-type`，返回「类型 + 启用字典项」 | 这是前端真正消费的形状，两套 CRUD 覆盖不到 |
| D6 | 级联删除 | 删类型时连带软删其下全部字典项 | GVA 不管，会留孤儿数据 |
| D7 | 唯一性口径 | 类型 `type` 唯一含软删占位；字典项 `value` 只约束活记录 | 见 §3.3，两类冲突不同 |
| D8 | 字段口径 | `status` 用 `i8`（沿用 `sys_dict`）；描述字段用 `remark` 而非 GVA 的 `desc` | `desc` 是 SQL 保留字；`remark` 与 `sys_api` 等既有表一致 |
| D9 | 类型编码字段名 | 用 `type`，Rust 侧写作 `r#type`，Column 为 `Column::Type` | 与 GVA 字段语义对齐；SeaORM 的 derive 支持裸标识符 |

## 3. 表结构

### 3.1 `sys_dictionary`（字典类型）

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | 主键 |
| name | VARCHAR(64) | NOT NULL DEFAULT '' | 字典名称（中文，展示用） |
| type | VARCHAR(64) | NOT NULL, UNIQUE | 字典类型编码（英文，业务键） |
| status | TINYINT | NOT NULL DEFAULT 1 | 1 启用 / 0 停用 |
| remark | VARCHAR(255) | NOT NULL DEFAULT '' | 备注 |
| created_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP | |
| updated_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE | |
| deleted_at | DATETIME | NULL | 软删时间 |

### 3.2 `sys_dictionary_detail`（字典项）

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | 主键 |
| dictionary_id | BIGINT UNSIGNED | NOT NULL | 所属类型 ID；不加外键，靠服务层保证 |
| label | VARCHAR(255) | NOT NULL DEFAULT '' | 展示值 |
| value | VARCHAR(255) | NOT NULL DEFAULT '' | 字典值（业务键） |
| extend | VARCHAR(255) | NOT NULL DEFAULT '' | 扩展值，前端可放 tag 颜色等 |
| sort | INT | NOT NULL DEFAULT 0 | 排序，升序 |
| status | TINYINT | NOT NULL DEFAULT 1 | 1 启用 / 0 停用 |
| created_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP | |
| updated_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE | |
| deleted_at | DATETIME | NULL | 软删时间 |

索引：

- `idx_sys_dictionary_detail_dictionary_id`（`dictionary_id`, `sort`）：列表与下拉查询；
- `sys_dictionary.type` 唯一键（含软删占位，与现有 `sys_dict.type_code` 行为一致）。

**不建** `(dictionary_id, value)` 数据库唯一键，原因见下节。

### 3.3 唯一性口径（D7 详解）

两处业务键的冲突场景不同，因此口径不同：

| 业务键 | 口径 | 理由 |
|---|---|---|
| 类型 `type` | 数据库 UNIQUE + 服务层查重**含软删** | 全局业务主键，沿用 `sys_dict.type_code` 既有行为，现有测试已覆盖「软删占位应拒绝重复」 |
| 字典项 `value` | **只约束活记录**，不加 DB 唯一键 | 删类型会级联软删一整组字典项；若软删仍占位，重建类型后无法再建同名 value，用户会莫名其妙 |

即：同一类型下不允许存在两个 `deleted_at IS NULL` 且 `value` 相同的字典项；
软删过的同 `value` 记录允许重建。

## 4. 数据搬迁

迁移 `m20260903_000009_create_sys_dictionary.rs` 的 `up` 分三步：

1. `create_table` 建 `sys_dictionary`、`sys_dictionary_detail` 与索引；
2. 搬迁存量数据（旧表 `type_code` 唯一，因此是严格的 1 : 1）：

```sql
INSERT INTO sys_dictionary (name, type, status, remark, created_at, updated_at, deleted_at)
SELECT type_code, type_code, status, remark, created_at, updated_at, deleted_at
FROM sys_dict;

INSERT INTO sys_dictionary_detail
  (dictionary_id, label, value, extend, sort, status, created_at, updated_at, deleted_at)
SELECT d.id, s.label, s.value, '', s.sort, s.status, s.created_at, s.updated_at, s.deleted_at
FROM sys_dict s
JOIN sys_dictionary d ON d.type = s.type_code;
```

3. `drop table sys_dict`。

`down` 只 drop 两张新表，**不还原** `sys_dict`（还原需要把 1:N 压回 1:1，会丢数据）。
搬迁是开发期的一次性动作，不需要可逆。

## 5. 接口契约

### 5.1 类型表：`POST /api/v1/dictionary/*`

| 端点 | 请求体 | 说明 |
|---|---|---|
| /list | `{ page, keyword?, status? }` | 分页；keyword 对 `name` / `type` 模糊；默认 `id` 倒序 |
| /create | `{ name, type, status, remark? }` | type 重复（含软删占位）返回 Biz 错误 |
| /update | `{ id, name, type, status, remark? }` | 全量覆盖；type 查重排除自身 |
| /get | `{ id }` | 详情 |
| /delete | `{ id }` | 软删类型 + 级联软删其下全部字典项 |
| /get-by-type | `{ type }` | 特殊契约端点，见 §5.3 |

### 5.2 字典项：`POST /api/v1/dictionary-detail/*`

| 端点 | 请求体 | 说明 |
|---|---|---|
| /list | `{ page, dictionary_id?, keyword?, status? }` | 分页；keyword 对 `label` / `value` 模糊；默认 `sort` 升序、`id` 升序 |
| /create | `{ dictionary_id, label, value, extend?, sort, status }` | 类型不存在返回 Biz；同类型活记录 value 重复返回 Biz |
| /update | `{ id, dictionary_id, label, value, extend?, sort, status }` | 全量覆盖；value 查重排除自身 |
| /get | `{ id }` | 详情 |
| /delete | `{ id }` | 软删单条 |

> 创建/更新均为编辑表单整体提交：`/create`（类型）的 `status`、`/create`（字典项）
> 的 `sort` 与 `status` 为**必填**（不用 DB 默认值），与 DTO 实现及本表一致。

### 5.3 `get-by-type` 响应形状

前端下拉真正消费的形状，一次请求拿全：

```json
{
  "code": 1,
  "data": {
    "id": 3,
    "name": "性别",
    "type": "gender",
    "details": [
      { "id": 7, "label": "男", "value": "1", "extend": "", "sort": 1 },
      { "id": 8, "label": "女", "value": "2", "extend": "", "sort": 2 }
    ]
  },
  "message": "ok"
}
```

规则：

- 只返回 `status = 1` 且未软删的字典项，按 `sort` 升序、`id` 升序；
- 类型不存在、已软删、或 `status = 0`，返回 Biz 错误
  `字典类型不存在或已停用：{type}`；
- 类型存在但没有任何可用字典项时，`details` 为空数组，**不报错**。

## 6. 模块组织

```
src/modules/dictionary/
  mod.rs        routes() + detail_routes()
  api.rs        类型组 CRUD → get-by-type → 字典项组 CRUD
  service.rs
  repo.rs
  dto.rs
```

函数命名：

| 层 | 类型表 | 字典项表 |
|---|---|---|
| repo | `find_dictionary_by_id` / `find_dictionary_page` / `create_dictionary` / `update_dictionary` / `soft_delete_dictionary` / `find_dictionary_by_type_include_deleted` | `find_detail_by_id` / `find_detail_page` / `create_detail` / `update_detail` / `soft_delete_detail` / `soft_delete_details_by_dictionary_id` / `find_enabled_details` / `find_alive_detail_by_value` |
| service | `page_dictionaries` / `create_dictionary` / `update_dictionary` / `get_dictionary` / `delete_dictionary` / `get_dictionary_by_type` | `page_dictionary_details` / `create_dictionary_detail` / `update_dictionary_detail` / `get_dictionary_detail` / `delete_dictionary_detail` |
| api | `list_dictionaries` / `create_dictionary` / `update_dictionary` / `get_dictionary` / `delete_dictionary` / `get_dictionary_by_type` | `list_dictionary_details` / `create_dictionary_detail` / `update_dictionary_detail` / `get_dictionary_detail` / `delete_dictionary_detail` |

`api.rs` 内顺序：类型组 `list → create → update → get → delete` → `get-by-type`
（特殊契约端点排在 CRUD 之后）→ 字典项组 `list → create → update → get → delete`。
`mod.rs` 的 `routes()` 路径为 `dictionary`，`detail_routes()` 路径为
`dictionary-detail`，两者在 `router.rs` 分别挂载（参照 `menu::user_routes()` 的
双路由范式）。

## 7. 错误处理

| 场景 | 错误 |
|---|---|
| 类型 / 字典项不存在（get / update / delete） | `字典类型不存在：{id}` / `字典项不存在：{id}` |
| 创建类型时 type 重复（含软删） | `字典类型编码已存在：{type}` |
| 创建 / 更新字典项时 value 重复（活记录） | `字典值已存在：{value}` |
| 创建 / 更新字典项时类型不存在 | `字典类型不存在：{dictionary_id}` |
| `get-by-type` 类型不存在或已停用 | `字典类型不存在或已停用：{type}` |

错误统一走 `AppError::Biz`，契约体 `code = 0`。

## 8. 菜单与权限码种子

在 `src/infra/seed.rs` 的 `MENU_SEEDS` 末尾追加（System 下第 7 位）：

| 项 | 值 |
|---|---|
| name | SystemDictionary |
| title | 数据字典 |
| path | /system/dictionary |
| component | #/views/system/dictionary/index.vue |
| icon | lucide:book-marked |
| menu_type | 2（页面） |
| parent | System |
| sort | 7 |

按钮权限（menu_type 3，parent = SystemDictionary）：

| name | title | permission | sort |
|---|---|---|---|
| SystemDictionaryCreate | 字典类型新增 | `system:dictionary:create` | 1 |
| SystemDictionaryUpdate | 字典类型编辑 | `system:dictionary:update` | 2 |
| SystemDictionaryDelete | 字典类型删除 | `system:dictionary:delete` | 3 |
| SystemDictionaryDetailCreate | 字典项新增 | `system:dictionary-detail:create` | 4 |
| SystemDictionaryDetailUpdate | 字典项编辑 | `system:dictionary-detail:update` | 5 |
| SystemDictionaryDetailDelete | 字典项删除 | `system:dictionary-detail:delete` | 6 |

查询入口由菜单本身控制，不单独配置 list 权限码。`MENU_SEEDS.len()` 断言会自动
跟随；super 角色全量绑定逻辑自动覆盖新增项。

## 9. 测试策略

### 9.1 repo 集成测试（直连 MySQL，测后清理）

- `find_dictionary_page`：keyword 命中 name / type、status 过滤、排除软删；
- `find_dictionary_by_type_include_deleted`：软删记录也能查到（验证唯一键含软删占位）；
- `find_detail_page`：`dictionary_id` 过滤 + keyword 命中 label / value + 排除软删；
- `find_enabled_details`：只返回 `status = 1` 且未软删，按 `sort` 升序；
- `soft_delete_details_by_dictionary_id`：批量软删后上述查询查不到；
- `find_alive_detail_by_value`：软删过的同 value 不被返回（验证可复用语义）。

### 9.2 service 测试

- `create_dictionary` 重复 type（活的 + 软删的）→ `AppError::Biz`；
- `update_dictionary` type 查重排除自身；
- `delete_dictionary` 级联软删其下字典项；
- `create_dictionary_detail`：类型不存在 → Biz；同类型活记录 value 重复 → Biz；
- 软删某字典项后，可重新创建同 value 的字典项（§3.3 的核心断言）；
- `get_dictionary_by_type`：正常返回；类型停用 → Biz；无可用字典项 → 空数组不报错；
- get / update / delete 不存在记录 → Biz。

### 9.3 种子测试

- `ensure_seed` 后 `SystemDictionary` 菜单页与 6 个权限码均存在且未软删；
- 总数断言跟随 `MENU_SEEDS.len()`，不硬编码。

## 10. 范围外

- 前端数据字典页面（前端仓库另行实现）；
- 字典缓存（W6 性能优化阶段，届时 `get-by-type` 可加缓存层）；
- 字典项批量导入 / 导出；
- codegen 支持一对多域定义（本轮手写四件套，生成器扩展另开议题）。

## 11. 参考实现位置

- 迁移注册：`migrations/src/lib.rs`
- 路由挂载：`src/infra/router.rs`
- 种子数据：`src/infra/seed.rs`
- 认证中间件：`src/middleware/auth.rs`
- 提取器与错误契约：`src/utils/request.rs`、`src/utils/response.rs`
- 分页通用件：`src/utils/page.rs`
- 现有单表实现（改造基准）：`src/modules/dict/`、`src/entity/sys_dict.rs`
- codegen 域定义目录：`codegen/defs/`（新增 dictionary.json、dictionary_detail.json）
