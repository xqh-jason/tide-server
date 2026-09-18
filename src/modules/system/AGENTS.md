## OVERVIEW

`src/modules/system/` — 18 个平台能力垂直切片（RBAC / 认证 / 字典 / 日志 / 任务 / 文件 / 组织）；四件套布局与挂载机制见父档，本档只写 18 切片共用的工程约定与逐域硬规则。

**建档理由**：得分 17（94 文件 / 18 子目录 / `SUPER_ROLE_KEY` 44 次、`enabled_int_values` 31 次等高中心化引用）。切片目录深度 4，不单独建档，逐域事实收在下面的表里。

## WHERE TO LOOK

18 个子目录深度 4，不单独建档；下表即本目录的结构清单（角色 + 一条硬规则）：

| 切片 | 角色 | 一条硬规则 |
|---|---|---|
| `user` | 用户 CRUD + 角色/部门/职位关联 | 内置 `admin` 不可编辑/删除/改状态；非 super 操作者不得把 super 角色绑给他人（防自我提权） |
| `role` | 角色 + 绑菜单/接口 | 改前必须先加载库中记录校验**原** `role_key`，否则超管角色可被改名转移绕过保留字检查 |
| `menu` | 菜单树（`parent_id` 邻接表） | 内存建树、`MAX_MENU_DEPTH = 10`；自环要单独拦（子孙集合不含自身，`contains` 判不出来） |
| `dept` | 部门树（`dept_path` 物化路径 `/0/{id}/`） | 环判定靠 `parent.dept_path.starts_with(...)`；`MAX_DEPT_TREE_DEPTH = 64`；移动子树逐层加锁 |
| `dictionary` | 字典类型 + 字典项（级联删） | `enabled_int_values(db, "status")` 是全仓状态取值唯一来源；删除返回级联条数供前端提示 |
| `sys_api` | 接口权限点登记 | 目录/表名带 `sys_` 前缀，与兄弟域裸名不一致；`uk_api_path_method` 保证命中至多一行 |
| `permission` | 接口授权判定 + 超管常量 + 按钮码查询（无 api/dto） | `SUPER_ROLE_KEY`、`ADMIN_USERNAME` 的唯一出处；后端授权只有 `has_api_permission` 一条通道（按钮码常量已删，勿重建） |
| `auth` | 登录 / 登出 / 刷新（无 repo） | 用户不存在、密码错、已禁用统一回「用户名或密码错误」防枚举；401 直写不走 `AppError` |
| `refresh_token` | 会话列表 / 强制下线 / 清理（无 validate） | 只允许删死记录（已吊销或已过期）；在线会话必须先强制下线，防「无痕踢人」绕过审计 |
| `captcha` | 图形验证码（无 repo） | 一次性消费：无论成败校验后立即删除防重放；不为此引入 `rand` |
| `file` | 上传 / 下载 | 落盘路径只由服务端 `stored_name` 拼装；`upload` 是 multipart 例外、`download` 是 GET 例外 |
| `config` | 参数配置 + 网站设置 | `sys_site_config` 恒单行 `id=1`，缺失即报「网站设置记录缺失，请执行迁移种子」 |
| `position` | 职位 | `position_code` 唯一键含软删占位，查重必须走 `find_by_code_include_deleted` |
| `job` | 定时任务 CRUD + `scheduler.rs` | DB 是事实来源：CRUD 提交后同步 add/remove，调度器失败不回滚 DB 只记 error（重启自愈） |
| `job_log` | 调度执行日志（只读 + 删） | 由 `job/scheduler.rs` 直写 repo；过期清理 `BATCH_SIZE = 1000` 滚动批删 |
| `login_log` | 登录日志 | 由 `auth/service.rs` 直写 repo（不经 service） |
| `operation_log` | 操作日志 | 由 `middleware/op_log.rs` 直写 repo；敏感键脱敏为 `***` |
| `health` | 健康检查（仅 api/mod） | `POST /api/v1/health`，公开档位，无 service/repo/dto |

## CONVENTIONS

### 分层与 repo「只拼 SQL」边界（18 切片通用）

repo 是数据访问原语层：把 SQL 原子化（过滤 / 排序 / 分页 / 审计盖章），不带业务判断，**不得调用其他域 repo 或 service**；需要他域语义时由 service 解析后传入参（`find_menus_by_role_ids(db, &role_ids)`，而非 repo 内部去查他域取角色）。Entity 全局共享，跨表查询允许在 repo 内直接用相关 Entity。跨域复用带业务规则的操作（唯一性校验、状态机、审计盖章、权限、软删策略）必须走该域 service。判断标准：**这段代码若换个业务场景就要改写，它就不该在 repo**。

- 允许留在 repo：动态过滤拼装（`if let Some(v) = filter.*`）；空集合短路（避免空 `IN`）；`find_*` / `find_*_include_deleted` 变体与关联表重建 `*_in_tx`；审计盖章与软删标记写入；写操作返回「是否命中」（`rows_affected > 0` / `bool`），存在性判断留给 service；入参约定的 `debug_assert`（编程错误用断言，不用业务错误表达）。
- 禁止（须上移 service）：业务保留字 / 权限判断（`RoleKey.ne(SUPER_ROLE_KEY)`）；业务错误文案（`AppError::Biz` / `anyhow!("xx不存在")`，repo 用 `Option` / `bool` 表达）；唯一性、重名、状态机等前置校验决策；「有子节点 / 有引用则拒删」策略判定；以业务语义决定是否执行某步（如「role_ids 为空则不清关联」）；跨域调用其它域 repo / service。
- 反例（2026-09-10 修正 3 类，现已落地）：① `role/repo.rs::find_all` 过滤超管 → 移至 `role/service.rs::get_all_roles`；② `dept/repo.rs::move_subtree_in_tx` / `soft_delete_dept_in_tx` 产出「部门不存在」文案 → 改返回 `Option` / `rows_affected > 0`；③ `user/repo.rs::update_user_in_tx` 用 `if !role_ids.is_empty()` 包住关联清空 → 无条件清空，「空数组即清空」由 service 语义决定。

### 并发一致性（加锁读，2026-09-14）

凡「读 → 判断 → 写」型不变式（防环 / 占用检查 / 引用挂载校验），读取必须走加锁读 `SELECT ... FOR UPDATE`（`.lock_exclusive()`）：RR 快照读下两个并发事务会各自基于旧结构决策、双双通过校验（写偏斜），成环 / 游离子树 / 悬挂引用都由此而来。

- repo 提供 `*_for_update` 原语，签名收 `&DatabaseTransaction`——autocommit 下单条语句执行完即释放，等于没加锁。
- 一个事务锁多行时**按 id 升序加锁**；子树下推 / 级联收集等遍历统一**自顶向下**（先父行、后子区间），与并发建子的插入意向锁同向。
- 跨域写入（角色绑菜单 / 接口、用户挂部门）同样先加锁读 + 存在性判定：只靠对方持有的间隙锁只挡插入时机，不判定仍会把已软删记录写进关系表（悬挂引用）。
- 加锁读在 RR 下会取间隙锁、可阻塞**相邻区间**插入：展示类查询（列表 / 详情 / 名称拼装）一律普通读，不要顺手加锁。
- 加锁顺序无法全局一致的极端交错仍可能 1213 / 1205，由 `utils/error.rs` 映射成「操作冲突，请稍后重试」兜底；不在应用层加超时（锁等待超时由 router 级超时中间件统一兜底）。
- 空集合不执行 DELETE（先 `count > 0` 守卫）：RR 下未命中的 `DELETE ... WHERE user_id = ?` 会在主键索引上加间隙锁，与并发插入意向锁互斥致 1213（已复现）。

### 软删除

主表逐查询手写 `.filter(Column::DeletedAt.is_null())`（无 sea-orm 软删插件）；关系表（`sys_user_role` / `sys_role_menu` / `sys_role_api` / `sys_user_dept` / `sys_user_position`）硬删除，但经关系表读主表仍须过滤主表软删条件。唯一键含软删占位 → 查重一律走 `find_by_*_include_deleted`。删除 = `update_many().col_expr(DeletedAt, Expr::value(Some(chrono::Local::now().naive_local())))`，以 `rows_affected > 0` 表达命中。

### 命名与顺序（切片内）

repo 分页 `find_page`、CRUD `find_by_id` / `create_*` / `update_*` / `soft_delete_*`；service `page_<实体>` / `create_<实体>` / `update_<实体>` / `get_<实体>` / `delete_<实体>`；handler `list_<实体>`。写关联表的 repo 函数加 `_with_links` 后缀。`api.rs` 函数顺序 = `mod.rs` 路由挂载顺序 = `list → create → update → get → delete`，特殊契约端点（`info` / `access-codes` / `menus` / `get-by-type` / `run-once`）排在 CRUD 之后。分页过滤参数打包成域 `*Filter`（定义在 `dto.rs`），与 `page_index` / `page_size` 分离——加条件只改 Filter，签名与调用点不动。

### 测试规范

- 集成测试内联在各域 `repo.rs` / `service.rs` / `validate.rs`，直连真实 MySQL、无 mock；纯单元测试在 `src/utils/`。
- 默认写法 = 事务回滚隔离：业务函数参数一律 `&impl ConnectionTrait`，夹具 `test_txn()`（真连接 + `begin()`，结束含 panic 时 `DatabaseTransaction` Drop 自动 ROLLBACK），不留孤儿数据、无需手写清理。
- **例外**：自持事务边界的 service 入口收 `&DatabaseConnection`——`create/update/delete_user`、`update_user_with_links`、`create/update/delete_role`、`create/update/delete_api`、`create/update/delete_menu`、`create/update/delete_dept`、`create/update/delete_position`、`delete_dictionary`。sea-orm 1.1.20 真库无 savepoint，事务内嵌套 `begin()` 会被 MySQL 隐式提交、破坏回滚隔离；这些用例仍用 `test_db()` 真连接 + 手写清理（函数注释有标注）。
- **并发用例例外**：需真实 `commit()` / 多连接才能复现的写偏斜与锁互斥，用 `test_db()` + 手工清理。优先**确定性交错**（一方先固定快照 → 另一方完整提交 → 前者再继续），不靠 sleep 竞态；真并发冒烟只断言与调度无关的不变量。清理必须在**结束事务之后**——加锁读持有的行锁 / 间隙锁会挡住自己的清理语句（表现为 50s 锁等待超时）。
- 测试数据唯一命名 `<prefix>_<pid>_<seq>`（各文件一个 `static SEQ: AtomicU64`），防撞真实库唯一键（`sys_user.username`、`sys_dictionary.type`）；service 用例固定加模块前缀，与 repo 用例数据永不相交。查询必须用关键字圈定范围，不依赖「全表只有本测试数据」这一不成立前提。
- 用例命名 `<行为>_<场景>`（如 `find_page_filters_by_keyword_and_status_excludes_deleted`）；跑单模块 `cargo test <模块名>`（如 `cargo test role`）。
- 人字段命名与名称拼装协议见 `src/utils/AGENTS.md`（唯一实现 `utils/user_ref.rs`），切片只负责 `From<Model> for *Resp` 与 `UserRefNames` impl。

## ANTI-PATTERNS

- 系统内置对象不可动：`admin` 用户与 `super` 角色的编辑 / 删除 / 改状态 / 改名转移全部拒绝。
- 业务层新增函数禁止新增 `db.begin()`：需要多表原子性时先把事务边界上移 service，并同步评估测试影响。
- 不用 SeaORM `#[transactional]` 或 `db.transaction(|txn| ...)` 闭包：统一「入口 `begin()` → 委托 `*_in_tx(&txn, ...)` → `if result.is_ok() { txn.commit() }`」。
- 四件套按域裁剪的实况（别照搬六文件）：`permission` 无 api/dto，`auth` / `captcha` 无 repo，`health` 只有 api/mod，`file` / 三个日志域 / `refresh_token` 无 validate，`job` 多一份 `scheduler.rs`。
- 树组装不做 N+1：一次全量查询 + `HashMap` 内存递归；遍历必须有终止条件（`visited: HashSet`）——MySQL 不允许 CHECK 引用自增列（错误 3818），自环无法在 DB 层拦截。
- 不用 `.count()` 做加锁存在性判定：sea-orm 会包成派生表，`FOR UPDATE` 落在内层不可靠；取行后由调用方判空。
