# 职位管理（position）— 主数据 + 用户多值挂载

日期：2026-09-12
目标：补齐「职务维度」主数据。新增 `position` 域（职位 CRUD）、`sys_user_position`
多值挂载，并在用户管理契约中展示用户职位。

## 决策记录

| 决策点 | 结论 | 说明 |
|---|---|---|
| 英文标识 | **`position`** | 表 `sys_position` / `sys_user_position`；权限码 `system:position:*`；路径 `/api/v1/position/*`；前端 `views/system/position/`。中文文案叫「职位管理」，全项目不再出现 `post` |
| 用户-职位关系 | **多值** `sys_user_position` | 一人可挂多个职位（兼任场景，如「董事 + 副总经理」）；关系表硬删，同 `sys_user_dept` |
| 业务编码 | 需要 `position_code` | 全局唯一且**含软删占位**（与 `sys_job.job_name` / `sys_config.config_key` 同款：软删行仍占用，不可重建同编码） |
| 主职位 | **不做** | 职位不参与权限、无「默认归属」需求，展示时全列即可（对比 `sys_user_dept.is_primary`） |
| 参与权限 | **不参与** | RBAC 功能权限看 `role`；数据权限看部门 + `is_leader`。职位是纯主数据 |
| 挂载校验 | 软删拒绝、**停用允许** | 与 user-dept 口径一致 |
| 挂载上限 | 20 | 兼任职位通常少；部门为 50 |
| 回显端点 | 需要 | `POST /api/v1/user/get-positions`（与 `get-depts` 对称，前端表单回显） |

## 现状（可复用）

- 新增域挂载 = `modules/mod.rs` DOMAINS 加一行；另补 `entity/mod.rs` 与 `modules/mod.rs` 的 pub mod
- user 域多部门那套写法可直接照搬：契约（`depts` → `positionIds`/`positions`）、
  结构与存在性校验、`replace_*_in_tx`（**无旧行跳过 DELETE**，避免 RR 间隙锁并发死锁）、
  批量名称拼装（查不到给空串）
- 平表 CRUD 可用 codegen（`codegen/defs/*.json`）生成骨架，再补 validate / 引用检查 / seed
- seed 幂等已加固：`sys_menu.name` 唯一索引 + 插入冲突回查；新菜单/API 直接加进
  `MENU_SEEDS` / `API_SEEDS` 即可

## 1. 数据模型（两条迁移，序号接 000023）

### m20260909_000024_create_sys_position

| 列 | 类型 | 说明 |
|---|---|---|
| id | BIGINT UNSIGNED AI | 主键 |
| position_code | VARCHAR(64) NOT NULL | 职位编码（全局唯一，含软删占位） |
| position_name | VARCHAR(64) NOT NULL | 职位名称 |
| sort | INT NOT NULL DEFAULT 0 | 排序 |
| status | TINYINT NOT NULL DEFAULT 1 | 1 启用 / 0 停用 |
| remark | VARCHAR(255) NOT NULL DEFAULT '' | 备注 |
| created_by / updated_by | BIGINT UNSIGNED NOT NULL DEFAULT 0 | 审计 |
| created_at / updated_at | DATETIME | 审计 |
| deleted_at | DATETIME NULL | 软删 |

唯一键：`uk_sys_position_code(position_code)`（含软删占位，软删行不可重建同编码）。

### m20260909_000025_create_sys_user_position

| 列 | 类型 | 说明 |
|---|---|---|
| user_id | BIGINT UNSIGNED | 复合主键之一 |
| position_id | BIGINT UNSIGNED | 复合主键之一，另建反查索引 |

关系表硬删（同 `sys_user_role` / `sys_user_dept`）。

## 2. position 域（平表 CRUD）

端点 `POST /api/v1/position/{list,create,update,get,delete}`（DOMAINS 登记
`path: "position"`, Protected）：

- `list`：**分页** + `keyword`（code/name 模糊）/ `status` / 审计过滤（非树，用 `PageResult`）
- `create` / `update`：code 查重（含软删占位）、name/code 长度、status 值域
- `get`：排除软删
- `delete`：**有用户引用（`sys_user_position`）→ 拒绝**；否则软删
- 写原语 `*_in_tx`；每行**只写变更列**（窄写）；审计字段由 repo 盖章
- Resp：含 `created_by_name` / `updated_by_name`（`UserRefNames` 协议）

## 3. user 域挂职位（多值）

- 契约：`Create/UpdateUserReq.positionIds: Vec<u64>`；`UserResp.positions:
  [{positionId, positionName}]`；缺省空数组 = 不挂/清空
- repo：`find_position_links_by_user_id` / `find_position_links_by_user_ids`（空入参短路）/
  `replace_user_positions_in_tx`（无旧行跳过 DELETE）
- service：`position` 域 `find_by_ids` 做存在性校验（差集报「职位不存在：{ids}」；
  软删拒绝、停用允许）；去重与上限 20 在 validate；
  关联维护在既有 create/update 事务内；名称批量拼装（软删职位给空串）
- 端点 `POST /api/v1/user/get-positions`（与 `get-depts` 对称）+ seed 登记

## 4. 权限码与 seed

- 菜单：`SystemPosition`（职位管理，`/system/position`，
  `#/views/system/position/index.vue`）+ 3 个按钮
  （`system:position:{create,update,delete}`）
- API 登记：`/api/v1/position/{list,create,update,get,delete}` +
  `/api/v1/user/get-positions`
- 说明：`system:position:list` 不单独做按钮（与 dept/role 一致，列表由页面 + API 授权控制）

## 5. 涉及文件

| 文件 | 改动 |
|---|---|
| `migrations/src/m20260909_000024_create_sys_position.rs` | 新建 |
| `migrations/src/m20260909_000025_create_sys_user_position.rs` | 新建 |
| `src/entity/sys_position.rs` / `sys_user_position.rs` | 新建 + `entity/mod.rs` 注册 |
| `src/modules/position/{api,dto,repo,service,validate}.rs` + `mod.rs` | 新建域 |
| `src/modules/mod.rs` | `pub mod position` + DOMAINS 登记 |
| `src/modules/user/*` | `positionIds` 入参 / `positions` 响应 / 校验 / 关联维护 / 拼装 / `get-positions` |
| `src/infra/seed.rs` | 菜单 + 按钮 + 6 条 API |
| `codegen/defs/position.json`（可选） | 若走 codegen 生成骨架 |

## 6. 测试（TDD：红 → 绿）

- position repo：分页过滤（keyword/status/审计、排除软删）、`find_by_code_include_deleted`、
  `count_user_refs_by_position_id`、create/update/soft_delete 窄写与盖章
- position service：code 重复拒绝（含软删占位）、删除有用户引用拒绝、删除成功、get 排除软删
- position validate：code/name 非空与长度、status 值域、remark 长度
- user repo：`replace_user_positions_in_tx`（重建 / 空数组清空 / 无旧行跳过 DELETE）、批量查询短路
- user service：职位不存在拒绝、软删拒绝、停用允许、去重、超 20、端到端落库、
  update 空数组清空、名称批量拼装（软删容错空串）
- user validate：`positionIds` 重复 / 超上限 / 含 0
- seed：职位菜单与按钮、API 登记齐全（幂等）

## 7. 验证

1. `cargo fmt --check`、`cargo check`
2. `cargo test position`、`cargo test modules::user`、`cargo test seed`
3. 服务验证：`cargo run` 后
   - `POST /api/v1/position/list` 无 token 返回未登录（挂载生效）
   - 库中确认 `sys_position` 表、`sys_user_position` 表、菜单/API seed 落库且幂等

## 8. 不做 / 后续

- 职级 / 薪资 / 审批流等 HR 业务（职位仅作主数据与展示）
- 职位与权限联动（明确不参与 RBAC 与数据权限）
- 「主职位」字段（多值纯展示，不需要默认归属）
- 前端职位管理页面（`salvo-vben-web`，本仓库只提供契约与菜单）

## 9. 落地进度（按批次）

### 批次 A（position 域 CRUD）— 待开始
### 批次 B（user 挂职位多值）— 待开始
