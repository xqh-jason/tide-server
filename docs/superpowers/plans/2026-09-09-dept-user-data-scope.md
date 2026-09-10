# 部门管理 + 用户多部门挂载 + 部门直控数据权限

日期：2026-09-09
目标：补齐组织架构主数据缺口。新增 `dept` 域（部门树 CRUD）、`sys_user_dept` 多对多
关系（含主部门与部门负责人位），并在**用户管理列表**上落地「部门直控」数据权限
（同一可见性模型未来可扩展到其它业务资源）。

## 决策记录（2026-09-09 逐轮澄清结论）

| 决策点 | 结论 | 说明 |
|---|---|---|
| 岗位 sys_post | 本轮不做 | 后续单列；届时再加 user-post 关联 |
| 用户-部门建模 | 多对多 `sys_user_dept` | 用户可挂多个部门；`user.dept_id` 单值方案作废 |
| 主部门 | 关系表 `is_primary` | 单点真源，不冗余到 sys_user，避免双写不一致 |
| 部门负责人 | 关系表 `is_leader` | 主管与成员同节点时，主管凭 is_leader 看本部门；可兼任多部门领导 |
| 同级互看 | `sys_dept.allow_peer_read`，默认 0 | 普通成员同部门默认互不可见，部门级独立放开 |
| 数据权限载体 | 用户部门直控 | 不引入「角色携带数据范围」；超管与接口级权限码不受行级范围影响 |
| 部门树结构 | `parent_id` + `dept_path` | 路径前缀为数据权限子树判断预留 |
| 运行时过滤 | 本轮纳入用户列表 | 部门直控闭环首落 `user/list` |

## 现状（装配点与复用）

- 新增域挂载 = `modules/mod.rs` DOMAINS 登记表加一行（module-mount-registry 已落地）；
  需另补 `entity/mod.rs` 与 `modules/mod.rs` 的 pub mod。
- user 域 create/update/delete 为**内部自带事务**（`&DatabaseConnection`，测试 `test_db()`
  例外），多部门关联维护放进该既有事务边界内，不新增 `db.begin()`。
- 人字段命名与 `UserRefNames` 拼装协议（`utils/user_ref.rs`）已就绪；部门名拼装
  仿同一模式但目标是 `sys_dept.dept_name`，只依赖 entity。
- 软删约定：`sys_dept` 软删；关系表 `sys_user_dept` 硬删（同 sys_role_menu）。
- 菜单/API/权限码 seed 集中在 `infra/seed.rs`；权限码常量在 `modules/permission`。

## 1. 数据模型（两条迁移，序号接 000017）

### m20260909_000018_create_sys_dept

| 列 | 类型 | 说明 |
|---|---|---|
| id | BIGINT UNSIGNED AI | 主键 |
| parent_id | BIGINT UNSIGNED NOT NULL DEFAULT 0 | 父部门，0 = 根 |
| dept_path | VARCHAR(255) NOT NULL DEFAULT '' | 段为从占位 `0` 到自身的整条链：根 `/0/{id}/`、子 `/0/父id/{id}/` |
| dept_name | VARCHAR(64) NOT NULL | 部门名 |
| sort | INT NOT NULL DEFAULT 0 | 排序 |
| status | TINYINT NOT NULL DEFAULT 1 | 1 启用 / 0 停用 |
| allow_peer_read | TINYINT NOT NULL DEFAULT 0 | 1 同部门普通成员互看 / 0 关 |
| remark | VARCHAR(255) NOT NULL DEFAULT '' | 备注 |
| created_by / updated_by | BIGINT UNSIGNED NOT NULL DEFAULT 0 | 审计 |
| created_at / updated_at | DATETIME | 审计 |
| deleted_at | DATETIME NULL | 软删 |

`dept_name` + `parent_id` 复合唯一（同父不可重名），**软删占位**（沿用 sys_config
语义）：软删行仍占用唯一键，同父同名不可重建，防历史引用歧义。

**字段收敛（2026-09-10 决策，迁移 m20260909_000020）**：删除原计划的 `leader` /
`phone` / `email` 三列。负责人由 `sys_user_dept.is_leader` 承载（**允许多个负责人**，
每部门多条 `is_leader=1`），展示由后端按 `is_leader` 拼装 `leaders: [{userId, name}]`；
`phone`/`email` 无任何消费方，按 YAGNI 删除。

### m20260909_000019_create_sys_user_dept

| 列 | 类型 | 说明 |
|---|---|---|
| user_id | BIGINT UNSIGNED | PK 之一，索引 |
| dept_id | BIGINT UNSIGNED | PK 之一，索引 |
| is_primary | TINYINT NOT NULL DEFAULT 0 | 主部门；每用户至多一个 1 |
| is_leader | TINYINT NOT NULL DEFAULT 0 | 本部门负责人位（可多人，不做唯一约束） |

关系表硬删；FK 到 sys_user / sys_dept。

## 2. dept 域（手写四件套，树逻辑超出 codegen 平表能力）

端点 `POST /api/v1/dept/{list,create,update,get,delete}`（DOMAINS 登记表加
`path: "dept", Protected`）。api 顺序 = 挂载顺序。

- `list`：部门树，children 递归，不分页；状态过滤参数可选。树深度上限防栈溢出（仿 menu）。
- `create`：parent 存在且未软删校验 → 插入 → 回写 `dept_path`（根 =
  `/0/{id}/`；子 = 父 path + `{id}/`）。
- `update`：普通改字段（名称、sort、开关等）；**parent_id 变更 = 移动**，
  事务内递归重算整棵子树 path（逐层 BFS：子 path = 新父 path + 自身 id + `/`）。
- 防环：parent 不得为自身或其子孙。
- `delete`：存在活子部门、或 `sys_user_dept` 存在活引用 → 拒绝；软删。
- 写原语统一 `*_in_tx(&DatabaseTransaction)`（同 role 域现行模式），测试用
  `test_txn()` 事务回滚隔离，无需手写清理；**每行只写变更列**（窄写），
  审计字段由 repo 统一盖章。
- Resp：含 `created_by_name/updated_by_name`（UserRefNames 协议）+ `children` +
  `leaders: [{userId, name}]`（批次 3 按 `sys_user_dept.is_leader` 批量拼装）。

## 3. user 域改造（多部门挂载）

- Create/Update 请求新增 `depts: [{ deptId, isPrimary, isLeader }]`（缺省空数组 =
  不挂部门，种子/管理员兼容）；列表过滤条件同步补 `deptId`。
- service 校验：部门存在且未软删；`depts` 非空时主部门有且仅有一个 `isPrimary`。
- 关联维护进既有 user create/update 事务：清 `sys_user_dept` 后整体重建。
- UserResp 新增 `depts: [{ deptId, deptName, isPrimary, isLeader }]`；deptName 批量
  拼装（收集 user_ids → 一次查关联 + 部门名 → 填充，查询不到空串）。
- 部门名拼装工具放 `utils/`（只依赖 entity），仿 `user_ref.rs` 但不归人字段协议。

## 4. 数据权限（部门直控，本轮闭环 `user/list`）

### 可见性规则

访问者 X（挂载部门集合 M_X）能看到目标用户 U，当且仅当满足其一：

1. `U == X`（本人恒可见）；
2. 存在 X 在部门 d 上 `is_leader=1`，且 U 挂在 d（主管看本部门，不受开关影响）；
3. `d.allow_peer_read=1`，且 X、U 都挂 d（开关放开，同级普通成员互看）；
4. X 挂在 d 的任意祖先部门 e（U 的 d 是 e 的严格下辖），`d ∈ subtree(e) \ {e}`；
5. 超管全量（复用系统现有超管判定，与接口权限码体系一致）。

普通成员默认：规则 1 命中本人；同部门同事需 2/3；跨部门靠 4 只对上级开放。

### 实现形态（不展开 user_id）

- 解析：`dept::service::resolve_scope(db, X) -> Scope`。Scope 携带两类条件：
  - 直接可见 dept_id 集合（规则 2/3 命中：含 leader 位或已开开关的挂载部门）；
  - 路径前缀集（规则 4：对 X 每个挂载部门 e，取其严格下辖子树，用
    `dept_path LIKE e.path || '%'` 表达或展开为后代 id 集，实现期二选一，以索引
    与测试为准）。
- 过滤：user_repo 分页主查询保持**不重复**（EXISTS 子查询判断目标用户至少一个挂载
  部门命中 Scope），辅以 `id = X.id` 白名单；超管跳过 Scope。
- 边界：X 无任何挂载部门 → 仅本人（除超管）。
- 本规则首落 `user/list`；其它资源（日志等）未来按同一模型扩展，届时审计类数据
  需按「写入时部门快照」落库，另行计划。

## 5. 权限码与种子

- `permission` 常量新增 `SYSTEM_DEPT_*`（list/create/update/delete）。
- `infra/seed.rs`：部门管理菜单（目录 + 页面 + 按钮）、`sys_api` 5 条端点资源；
  部门树种子数据（若需演示，含开关样例）。

## 6. 涉及文件

| 文件 | 改动 |
|---|---|
| `migrations/src/m20260909_000018_create_sys_dept.rs` | 新建 |
| `migrations/src/m20260909_000019_create_sys_user_dept.rs` | 新建 |
| `migrations/src/m20260909_000020_drop_sys_dept_leader_phone_email.rs` | 新建（字段收敛） |
| `src/entity/` sys_dept.rs / sys_user_dept.rs | 新建（sea-orm-codegen） |
| `src/entity/mod.rs` | 注册 |
| `src/modules/dept/{api,dto,service,repo,validate}.rs` + mod.rs | 新建域 |
| `src/modules/mod.rs` | pub mod dept + DOMAINS 登记一行 |
| `src/modules/user/*` | depts 入参/校验/关联维护/Resp/部门名拼装/list Scope 过滤 |
| `src/modules/permission/mod.rs` | SYSTEM_DEPT_* 常量 |
| `src/infra/seed.rs` | 菜单 / API / 演示数据 |
| `src/utils/` | dept 名批量拼装辅助 |

## 7. 测试（TDD：红 → 绿）

- dept：创建回写 path / 深层 path / 移动重算子树 / 防环 / 删父拒绝（有子、有挂载
  用户）/ 软删 / 树 list / 同层重名冲突。
- user 多部门：重建关联、主部门唯一校验、is_leader 透传、非法/软删部门拒绝、
  Resp dept 名拼装。
- 数据权限：规则 1-5 逐条（本人 / 主管本部门 / 开关开同级互看 / 上级祖先可见 /
  超管放行）、多部门用户出现在多子树并集、多部门用户去重、无挂载回退仅本人、
  跨区隔离（华东不可见华北）。
- dept 写路径（create/update/move/soft_delete）与 user 关联重建一律 `*_in_tx` +
  测试外层 `test_txn()` 事务回滚隔离，无需手写清理。

## 8. 验证

1. `cargo fmt --check`
2. `cargo check`
3. `cargo test`（需本地 MySQL：`docker compose up -d`）
4. 行为抽查：dept 树 CRUD + 移动后 dept_path 正确；用户多部门挂载/主部门切换；
   以不同部门挂载账号调 `user/list` 断言可见集合符合 5 规则。

## 9. 不做 / 后续

- 岗位 sys_post（后单列，届时 user-post 关联）。
- 数据权限扩展到日志等历史事实表（需先立「写入时部门快照」约定）。
- codegen 树模板扩展、部门树批量导入导出。
- 「角色携带数据范围」经典 GVA 模型（当前选择用户部门直控，如未来切换需另计划）。

## 10. 落地进度（按批次）

### 批次 1（2026-09-09 完成）
- 迁移 `m20260909_000018_create_sys_dept` 已应用；`sys_dept` 唯一键
  `uk_sys_dept_parent_name(parent_id, dept_name)`（软删占位）。
- `entity/sys_dept.rs` + 注册；`modules/dept` 骨架。
- `dept/repo.rs` 8 个原语实现完成，**repo 层 10 测试全绿**：
  建树回写 path（根 `/0/{id}/`、子链式）、同父同名拒绝、软删占位（不可同名重建）、
  移动子树 path 重算、审计盖章、find 排除软删。

### 批次 2（2026-09-09 红态就绪，待实现）
- 迁移 `m20260909_000019_create_sys_user_dept` 已应用：复合主键
  `(user_id, dept_id)` + `is_primary` / `is_leader` + `dept_id` 反查索引。
- `entity/sys_user_dept.rs` + 注册。
- `dept/dto.rs`（Create/Update Req、DeptResp 树节点 + UserRefNames）。
- `dept/repo.rs` 新增 `count_user_refs_by_dept_id`（stub，待实现）。
- `dept/service.rs` 5 个业务函数（stub）+ **14 个红测**：父存在性/软删父拒绝、
  同父同名友好错误、防环（自身/子孙）、合法移动端到端、字段更新不移动、
  删除有子部门/有用户引用拒绝、删叶子成功、get 排除软删、树组装含停用节点。
- `move_subtree_in_tx` 优化（窄写 + 审计盖章、visited 防御、入参约定断言、
  no-op 短路、`path_map` 防御式取值），补 4 个测试（盖章 / no-op / 脏环终止 / 宽树）。
- **字段收敛**（2026-09-10）：迁移 `m20260909_000020` 删除 `leader`/`phone`/`email`，
  实体与 dto 同步；负责人多值语义确认（`is_leader` 多条）。
- 当前状态：`cargo test dept::repo` → **14 passed / 0 failed**；
  service 5 个业务函数 + `count_user_refs_by_dept_id` 仍为 stub（14 红待实现）。

### 后续批次（待排）
- 批次 3：`dept/api.rs` + `validate.rs` + DOMAINS 登记挂载 + permission 常量/seed +
  `leaders` 拼装（按 `sys_user_dept.is_leader`）
- 批次 4：user 域多部门改造（`depts` 入参 / 关联重建 / Resp 部门名）
- 批次 5：数据权限运行时过滤（5 规则，`user/list`）
