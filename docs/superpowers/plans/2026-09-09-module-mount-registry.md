# 模块挂载登记表化（消除新增模块的 3 处手工装配）

日期：2026-09-09
目标：当前 15 个域，新增一域需手工补 `entity/mod.rs` + `modules/mod.rs` + `infra/router.rs`
三处清单；router.rs 内 11 次重复 `.hoop(AuthRequired).hoop(OperationLog).hoop(ApiPermission)`。
重构为「域挂载登记表 + 循环挂载」，未来新增 = 登记表加一行。

## 现状（装配点）

`infra/router.rs::build` 里每个业务域重复：

```rust
.push(Router::with_path("menu")
    .hoop(AuthRequired).hoop(OperationLog).hoop(ApiPermission)
    .push(crate::modules::menu::routes()))
```

且出口形态不统一：
- `system::routes()`、`auth::routes()` 内部自带头 path（`health` / `auth`），router.rs 直接 push；
- 其余域出口为纯子路径（list/create/...），由 router.rs 外层 `with_path` 包前缀；
- `user` 前缀下并挂两个出口：`user::routes()` + `menu::user_routes()`（menu 域实现菜单契约端点）。

## 设计

### `src/modules/mod.rs` 新增登记表

```rust
/// 域出口挂载方式：公开 / 受保护（登录 + 操作日志 + 接口级授权三件套）。
#[derive(Clone, Copy)]
pub enum MountGuard {
    Public,
    Protected,
}

/// 一条挂载记录：path 为 api/v1 下的前缀；空串表示直接挂在 api/v1（出口自带 path）。
/// 一个前缀可并挂多个出口（如 /user 下 user CRUD + menu 菜单契约端点）。
pub struct DomainMount {
    pub path: &'static str,
    pub guard: MountGuard,
    pub routers: &'static [fn() -> salvo::prelude::Router],
}

/// 业务域挂载登记表：新增域 = 此表加一行（另需 entity/mod.rs、本文件 pub mod）。
/// 顺序与注释尽量贴合 router.rs 原挂载顺序，便于 diff 对照。
pub const DOMAINS: &[DomainMount] = &[
    // 健康检查（system 域，出口自带 /health）
    DomainMount { path: "", guard: MountGuard::Public, routers: &[system::routes] },
    // 图形验证码：公开（登录前调用）
    DomainMount { path: "captcha", guard: MountGuard::Public, routers: &[captcha::routes] },
    // 登录/登出：出口自带 /auth 前缀
    DomainMount { path: "", guard: MountGuard::Public, routers: &[auth::routes] },
    // 用户管理 + 菜单契约端点（/user/menus）
    DomainMount { path: "user", guard: MountGuard::Protected, routers: &[user::routes, menu::user_routes] },
    DomainMount { path: "menu", guard: MountGuard::Protected, routers: &[menu::routes] },
    DomainMount { path: "dictionary", guard: MountGuard::Protected, routers: &[dictionary::routes] },
    DomainMount { path: "dictionary-detail", guard: MountGuard::Protected, routers: &[dictionary::detail_routes] },
    DomainMount { path: "role", guard: MountGuard::Protected, routers: &[role::routes] },
    DomainMount { path: "sys-api", guard: MountGuard::Protected, routers: &[sys_api::routes] },
    DomainMount { path: "operation-log", guard: MountGuard::Protected, routers: &[operation_log::routes] },
    DomainMount { path: "login-log", guard: MountGuard::Protected, routers: &[login_log::routes] },
    DomainMount { path: "file", guard: MountGuard::Protected, routers: &[file::routes] },
    DomainMount { path: "config", guard: MountGuard::Protected, routers: &[config::routes] },
    DomainMount { path: "job", guard: MountGuard::Protected, routers: &[job::routes] },
    DomainMount { path: "job-log", guard: MountGuard::Protected, routers: &[job_log::routes] },
    // 网站设置：GET 公开，POST 鉴权由子路由自挂（config::site_routes）
    DomainMount { path: "site-config", guard: MountGuard::Public, routers: &[config::site_routes] },
];
```

说明：
- `path: ""` 两条（system / auth）保持现状「直接 push 到 api/v1」，因出口自带 path；
- Protected 三件套中间件顺序与现状一致：AuthRequired → OperationLog → ApiPermission；
- DOMAINS 内引用各域 `routes()`，全部为 `pub fn() -> Router`，函数指针可入数组。

### `src/infra/router.rs` 循环化

`build()` 中 `Router::with_path("api/v1")` 的 push 主体改为遍历 DOMAINS：

```rust
fn mount_domains(api: Router) -> Router {
    use crate::modules::{DomainMount, MountGuard, DOMAINS};
    use crate::middleware::{api_permission::ApiPermission, auth::AuthRequired, op_log::OperationLog};

    let mut api = api;
    for mount in DOMAINS {
        let mut inner = Router::new();
        if matches!(mount.guard, MountGuard::Protected) {
            inner = inner
                .hoop(AuthRequired)
                .hoop(OperationLog)
                .hoop(ApiPermission);
        }
        for routes in mount.routers {
            inner = inner.push(routes());
        }
        api = if mount.path.is_empty() {
            api.push(inner)
        } else {
            api.push(Router::with_path(mount.path).push(inner))
        };
    }
    api
}
```

`InjectState` hoop 保留在 Router::new() 顶层，`api/v1` 前缀结构不变。
OpenAPI 的 `merge_router(&router)` 在 build 返回后由 run() 调用，行为不变。

## 等价性论证

- 每行挂载的 path 前缀、中间件组合、出口集合与原 router.rs 逐条一致，无路由增删；
- hoop 顺序保持一致，鉴权/日志语义不变；
- 原 `api/v1` Router 内的 push 顺序由 DOMAINS 数组顺序承载（顺序只影响路由匹配优先级，各出口 path 互不重叠，无实际影响）。

## 涉及文件

| 文件 | 改动 |
|---|---|
| `src/modules/mod.rs` | 新增 `MountGuard` / `DomainMount` / `DOMAINS`（挂载 doc 注释同步更新） |
| `src/infra/router.rs` | build() 主体改为遍历 DOMAINS，原 push 清单删除 |

不改：域四件套、entity、middleware、codegen、seed。

## 不做（本轮明确排除）

- 二级业务分组目录：域数 < ~25 且无第二业务块前不做，避免 35+ 处跨域引用全部加前缀；
- codegen 模板联动（未来可让 def.json 带 mount 元数据，生成器自动写登记行）；
- 薄日志域合并（表不同，代码量主体是测试，无实际重复可消）。

## 验证

1. `cargo fmt --check`
2. `cargo check`
3. `cargo test`（需本地 MySQL：`docker compose up -d`）
4. 行为抽查：启动 `cargo run` 后
   - `POST /api/v1/auth/login` 正常（公开出口仍可达）；
   - `GET /api/v1/health` 返回 ok；
   - `POST /api/v1/user/list` 带 token 通过、不带 token 被 AuthRequired 拦截（Protected 三件套仍生效）。
