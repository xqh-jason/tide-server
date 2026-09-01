# Rust 学习打卡记录

> 规则：每天 1 分钟记录「目标 / 完成 / 卡点 / 明日」；周日对照计划文档 §8 里程碑复盘（M1–M7 打勾）。
> 只记"解决了什么 / 卡在哪"，不写流水账。

---

## 打卡记录

### 2026-08-26（W2 收尾）

- 目标：W2 认证与授权闭环（登录 / 权限码 / 菜单 / token 黑名单）。
- 完成：argon2 密码哈希、JWT 签发校验（jsonwebtoken 10 + rust_crypto）、Cache trait 内存实现、认证中间件、login/logout/info/access-codes/menus 五个端点；17/17 测试全绿；提交 `3aa80a1`。
- 卡点：service 层用 anyhow 会把 `AppError::Biz` 吞成 500 → 业务错误改用 `Result<T, AppError>`；jsonwebtoken 10 必须显式指定加密提供者 feature。
- 明日：复盘 W2 学习点（trait object / 错误处理 / 提取器），按自己节奏重写关键函数。

---

## 学习笔记：Rust 内存分配器调研（W7 决策用）

> 来源：Salvo 官方文档 jemallocator 篇 + 社区实践调研，2026-08-26 记录。

### 背景

- Salvo 官方文档 Tip：默认分配器（glibc ptmalloc）在长运行高并发服务中"有时不能及时释放内存"，推荐替换为 jemalloc。
- 新项目用 **tikv-jemallocator**（0.6+ 活跃维护，0.7.0 于 2026-05 发布），非 MSVC 平台可用。
- 社区证据：jemalloc 论文称 4 核服务器吞吐约为 glibc malloc 的 6 倍；生产使用者有 TiKV / RocksDB / Redis / near / uv。
- 反方权衡：jemalloc 有 5–10% 后台线程开销；不是所有场景更快；rustc 曾内置后又移除，为平台兼容与工具链灵活性。

### 当前结论（学习期）

- **不替换**。当前低并发开发环境，无性能问题；引入 jemalloc 需要 C 工具链（Docker 构建）、有镜像缺包风险（同 salvo_extra 教训）、增加平台约束。
- 决策点放在 W7：先测默认分配器基线，再 A/B 对比 jemalloc，有数据支撑才换。

### W7 A/B 验证流程

1. 默认分配器基线：`cargo run --release` 起服务，压测（记录吞吐、P99、RSS）。
2. 观察指标：RSS 只涨不落（内存不释放）、P99 抖动大 → 有理由试 jemalloc。
3. 接入 jemalloc（代码骨架）：

```toml
# Cargo.toml（先确认 rsproxy 镜像有对应版本，缺则换源）
[target.'cfg(not(target_env = "msvc"))'.dependencies]
tikv-jemallocator = "0.7"
```

```rust
// main.rs（main 函数之前）
#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
```

4. 同压测复跑，对比：RSS 是否回落、P99、吞吐。
5. 决策：收益明显则保留并写进 README 构建说明；不明显则回滚并记录原因。

### 备选方案

- 若 profiling 显示**分配延迟高**而非内存碎片 → 试 mimalloc（另做 A/B）。
- 判断口诀：碎片/内存不释放 → jemalloc；分配延迟/吞吐瓶颈 → 先看 mimalloc。
- 不要凭感觉换，压测数据说话。

---

## 2026-08-27（W3 RBAC 学习决策）

### 学习原则

> 业务实现由我手动完成；测试由 AI 统一编写和维护，并在实现完成后 review。

### 软删除策略

- 主表使用软删除：`sys_user`、`sys_role`、`sys_menu`、`sys_api`
- 关系表使用硬删除：`sys_user_role`、`sys_role_menu`、`sys_role_api`
- 所有业务查询默认过滤主表 `deleted_at IS NULL`
- 关系表不需要自身软删除条件，但查询其关联主表时必须过滤主表 `deleted_at`

### W3 第一步：软删除口径补齐

计划先写失败测试：

- deleted user 不允许登录
- deleted user 不能通过 `/user/info` 查到
- 用户分页不显示已删除数据
- deleted role 不能分配给新用户
- 登录后的 roles 不包含 disabled / deleted role

---

## 2026-08-27（W3 协作与边界测试）

### 协作调整

> 用户修改完代码后，后续测试由 AI 编写；业务实现仍由用户手动调整。AI 完成后负责 review 和验证。

### 已补测试

- `get_user_info_rejects_deleted_user`
- `create_user_rejects_username_matching_soft_deleted_user`
- `create_user_rejects_duplicate_role_ids`
- `create_user_accepts_unsorted_distinct_role_ids`

### 当前结果

- 前三个边界测试已通过。
- 第四个失败：传入不同但未排序的 `role_ids` 时被误判为“角色ID重复”。
- 结论：重复校验应比较“去重后的数量”或“相邻元素是否相等”，不能把排序后的集合与原顺序直接比较。

---

## 2026-08-27（W3 权限方案确认）

### 方案选择

> 采用“按钮权限码 + 后端显式校验”作为第一版权限闭环。

- `sys_menu.permission` 作为第一版权限码主数据。
- 前端用 `/access-codes` 控制按钮显隐；后端仍必须独立校验同一权限码。
- `system:user:create` 是第一个受保护操作权限。
- `super` 基于数据库实时有效角色短路放行，不信任 JWT 旧角色快照。
- `sys_api` / `sys_role_api` 保留表结构，暂不参与第一版主授权链路。

### 新增计划

- 项目内计划：`docs/superpowers/plans/2026-08-27-w3-permission-codes.md`
- 协作方式：AI 编写测试，用户实现业务代码，AI 最终 review 和验证。

### 当前验证

- 已在项目模块注释中固化权限码约定。
- `cargo fmt --check` 通过。
- `cargo test` 通过：28 passed / 0 failed。

---

## 2026-08-31（W3 角色域收尾 + 契约统一 + 重构）

- 目标：完成角色域 CRUD，统一响应契约，做一轮全代码优化检查。
- 完成：
  - 角色域四件套 + `POST /api/v1/role/{list,create,update,get,delete}` 五个端点（认证保护），
    创建/更新在同一事务维护 `sys_role_menu` / `sys_role_api` 关联（全量替换、空数组清空）。
  - 响应体契约统一：`code = 1` 成功 / `code = 0` 失败，HTTP 一律 200，仅鉴权失败保留 401。
  - 分页重构：新增 `PageData<T>` + 泛型转换 + 通用 `paginate<E>()`，repo 分页机械动作收敛到一处，
    各域 dto 删除重复的 `From` 实现。
  - 优化修复：登录拒绝禁用用户（防枚举）、logout TTL 防御、`from_depot` 错误改 `Internal`、
    菜单树深度上限（防超深链递归）、接口命名与挂载顺序统一（list → create → update → get → delete）。
  - 测试 57/57 全绿，`cargo fmt --check` 通过；提交已由用户完成。
- 卡点：
  - `role_key` 唯一索引与软删除冲突：同 key 两条记录（正常 + 软删）物理插不进去，
    测试改为两个 key 分别验证"正常占位 / 软删占位"。
  - SeaORM 泛型 `paginate` 需要 `E::Model: FromQueryResult + Send + Sync` bound，
    且 `PaginatorTrait` 必须显式导入（曾误删导致方法解析失败）。
  - repo / service 错误分层：repo 用 `anyhow` 传播技术错误，service 用 `AppError` 分类业务错误，
    `#[from] anyhow::Error` 自动把技术错误归为 `Internal`；业务校验放 service，否则会被吞成 500。
- 明日：菜单域 CRUD——保留 `/user/menus` 契约，新增菜单管理端点，补普通用户按 `sys_role_menu`
  过滤菜单树（当前 TODO），按钮权限码同源生效。

---

## 2026-09-01（W3 收尾：菜单/API 权限域 + 种子数据，M3 达成）

- 目标：完成 W3 剩余菜单域、API 权限域与种子数据，达成 M3（MVP）。
- 完成：
  - 菜单域四件套 + `POST /api/v1/menu/{list,create,update,get,delete}`；`/user/menus` 普通用户
    按 `sys_role_menu` 实时过滤（超管全量）；删除菜单级联软删全部子孙。
  - API 权限域四件套 + `POST /api/v1/sys-api/{list,create,update,get,delete}`，
    创建/更新在同一事务维护 `sys_role_api` 授权（全量替换）；`path + method` 查重含软删。
  - 分页过滤统一为域 `*Filter` 结构体（UserFilter / RoleFilter / MenuFilter / ApiFilter），
    repo 签名不再随过滤条件变化。
  - 种子数据 `ensure_seed`：admin（admin123）/ super 角色 / 17 条默认菜单树 /
    12 个按钮权限码 / RBAC 绑定，启动幂等执行；数据库测试残留已清理。
  - 测试 84/84 全绿，`cargo fmt --check` 通过。
- 卡点：
  - 种子"先查后插"在测试并发下不幂等（TOCTOU）→ `OnceLock<tokio::sync::Mutex>` 串行化。
  - 菜单 `get_menu` 曾把菜单 id 当 user_id 调 `get_menus`（返回菜单树而非详情）——
    review 抓到后改为 `service::get_menu`。
  - API 域曾走裸 `create_api/update_api`（只写主表），`role_ids` 授权被静默丢弃；
    改为 `with_roles` 版本并删除绕过路径。
- 明日：W4 代码生成器——复用通用 `paginate` / `PageData` / `*Filter` 模式，
  从域定义生成 entity + 四件套，并用它产出 W5/W6 模块骨架。
