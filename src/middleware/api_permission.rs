//! 接口级授权中间件：按 `path + method` 匹配 `sys_api` 并校验角色授权。
//!
//! # 挂载位置与顺序
//!
//! 挂在各受保护域组上、`OperationLog` 之后：`AuthRequired → OperationLog → ApiPermission`。
//! - 依赖 `AuthRequired` 写入的 [`AuthUser`]，未登录请求不会到达本中间件；
//! - 排在 `OperationLog` 之后是有意的：授权失败的写操作仍会留操作日志
//!   （审计最需要记录的就是被拒请求）。
//!
//! # 判定与响应契约
//!
//! 判定内核在 `permission::service::has_api_permission`，本中间件只做薄壳：
//! 取状态 → 取当前用户 → 调判定 → 渲染失败响应。
//! 拒绝时返回 HTTP 200 + `{"code":0,"data":null,"message":"无该接口访问权限"}`
//! 并 `ctrl.skip_rest()`——项目契约 HTTP 状态码一律 200（仅认证失败 401 例外），
//! 因此不使用 403，前端按 `code = 0` 与 `message` 识别无权限。
//!
//! 公开路由（health / login / captcha / site-config/get）与 `auth/logout`、
//! `site-config/update` 不挂本中间件，由路由组装层控制。
//!
//! # 路径规范化（2026-09-17 修，勿回退）
//!
//! 查 `sys_api` 前必须把请求路径规范化到「登记时的写法」，否则路由与判定不一致：
//! Salvo 路由按 percent-decoded、折叠重复斜杠、忽略尾斜杠的 path part 匹配
//! （`salvo_core::routing::parse_path_parts`），而 `req.uri().path()` 是原文——
//! `/api/v1/user/create/`、`//create`、`/%63reate` 都能进 handler，却查不到登记行，
//! 从而落到 fail-open 分支被放行（实测零授权用户借此建出用户）。
//! 规范化规则与路由一致：逐段 percent-decode → 丢弃空段 → 重新以 `/` 拼接，
//! 故尾斜杠 / 重复斜杠 / 转义在判定面前等价，且与 `sys_api.path` 的登记写法
//! （`/api/v1/...`，无尾斜杠）对齐。

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::utils::response::ApiResponse;

/// 把请求路径规范化成 `sys_api.path` 的登记写法。
///
/// 与 Salvo 路由的匹配口径保持一致（见模块文档）：先对整串做 percent-decode
/// （非法转义原样保留），再按 `/` 切段丢弃空段，最后拼回带前导 `/`、无尾斜杠的路径。
pub(crate) fn canonical_path(raw: &str) -> String {
    let decoded = salvo::routing::decode_url_path(raw);
    let mut out = String::with_capacity(decoded.len());
    for part in decoded.split('/') {
        if part.is_empty() {
            continue;
        }
        out.push('/');
        out.push_str(part);
    }

    // 全部段为空（如请求 `/`）时回退为单个 `/`，避免返回空串。
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// 接口级授权中间件：挂在受保护路由组上（`AuthRequired` 之后）。
///
/// fail-open 语义：`sys_api` 未登记的 `path + method` 一律放行，只有已登记
/// 接口才强制 `sys_role_api` 角色授权（超管短路），接口登记按域逐步接管。
pub struct ApiPermission;

#[async_trait]
impl Handler for ApiPermission {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        let state = depot.get_typed::<AppState>().ok();
        let auth = AuthUser::from_depot(depot).ok();

        // 两者缺失均属装配/顺序错误（本中间件必须挂在 InjectState 与 AuthRequired
        // 之后），记 error 日志并按拒绝处理：授权层故障宁可拒绝也不放行（fail-closed），
        // 与判定层的 fail-open（未登记放行）语义不同。
        let (Some(state), Some(auth)) = (state, auth) else {
            tracing::error!("app state or auth user missing before api permission middleware");
            deny(res, ctrl);
            return;
        };

        // call_next 前拷贝 path 与 method，避免 &mut Request 借用冲突（参考 op_log.rs）
        // path 必须规范化后再查 sys_api：路由按解码/折叠斜杠匹配，原文查会漏 → fail-open
        let path = canonical_path(req.uri().path());
        let method = req.method().as_str().to_string();

        match crate::modules::system::permission::service::has_api_permission(
            &state.db,
            auth.user_id,
            &path,
            &method,
        )
        .await
        {
            Ok(true) => {}
            // 未授权拒绝；技术错误记日志后同样拒绝（fail-closed），不放行
            Ok(false) => deny(res, ctrl),
            Err(err) => {
                tracing::error!("api permission check failed: {err}");
                deny(res, ctrl);
            }
        }
    }
}

/// 渲染授权失败响应（HTTP 200 + `{code:0, data:null, message}` 契约体，
/// 不使用 403——项目契约仅认证失败 401 例外）并跳过后续 handler。
fn deny(res: &mut Response, ctrl: &mut FlowCtrl) {
    res.render(Json(ApiResponse::<()>::fail("无该接口访问权限")));
    ctrl.skip_rest();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_role, sys_role_api, sys_user, sys_user_role};
    use crate::utils::cache::MemoryCache;
    use sea_orm::ActiveValue::Set;
    use sea_orm::Database;
    use sea_orm::entity::prelude::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    async fn app_state(db: DatabaseConnection) -> AppState {
        let config = crate::infra::config::Config::load().unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        AppState::new(config, db, Arc::new(MemoryCache::new()), scheduler)
    }

    /// 跑一次 ApiPermission（受保护链路专用：真库 + Depot 里已有 AuthUser）。
    /// 返回 (响应, 是否放行到后续 handler)。
    async fn run_permission(
        state: AppState,
        user_id: u64,
        path: &str,
        method: &str,
    ) -> (Response, bool) {
        let mut req = Request::new();
        req.set_uri(
            format!("http://test{path}")
                .parse::<salvo::http::uri::Uri>()
                .unwrap(),
        );
        *req.method_mut() = method.parse().unwrap();
        let mut depot = Depot::new();
        depot.insert_typed(state);
        depot.insert_typed(AuthUser {
            user_id,
            roles: vec![],
            refresh_token_id: 0,
        });
        let mut res = Response::new();
        let mut ctrl = FlowCtrl::new(vec![Arc::new(ApiPermission)]);
        ctrl.call_next(&mut req, &mut depot, &mut res).await;
        let allowed = !ctrl.is_ceased() && res.body_mut().is_none();
        (res, allowed)
    }

    async fn seed_user(db: &impl ConnectionTrait) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique("apimw_user")),
            password: Set("x".to_string()),
            nickname: Set("接口授权中间件测试用户".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(db: &impl ConnectionTrait) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("apimw_role")),
            role_key: Set(unique("apimw_role_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 登记一个生效接口（path 用调用方给的原文，用于构造“登记写法即规范写法”的场景）。
    async fn seed_api(db: &impl ConnectionTrait, path: &str) -> sys_api::Model {
        sys_api::ActiveModel {
            path: Set(path.to_string()),
            method: Set("POST".to_string()),
            description: Set("接口授权中间件测试".to_string()),
            api_group: Set("apimw_test".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 清理（自持连接无事务回滚，手工删干净）。
    async fn cleanup(db: &DatabaseConnection, user_id: u64, role_id: u64, api_id: u64) {
        sys_user_role::Entity::delete_many()
            .filter(sys_user_role::Column::UserId.eq(user_id))
            .exec(db)
            .await
            .unwrap();
        sys_role_api::Entity::delete_many()
            .filter(sys_role_api::Column::RoleId.eq(role_id))
            .exec(db)
            .await
            .unwrap();
        sys_api::Entity::delete_by_id(api_id)
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_by_id(role_id)
            .exec(db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(user_id)
            .exec(db)
            .await
            .unwrap();
    }

    // ===== 路径规范化（2026-09-17 回归前置：曾可直接绕过授权）=====

    /// 规范化口径：尾斜杠 / 重复斜杠 / 转义与原路径等价；非法转义原样保留。
    /// 与 Salvo 路由的实际规范化结果对齐（不可照抄其他实现的额外规则）。
    #[test]
    fn canonical_path_matches_router_normalization() {
        assert_eq!(canonical_path("/api/v1/user/create"), "/api/v1/user/create");
        assert_eq!(
            canonical_path("/api/v1/user/create/"),
            "/api/v1/user/create"
        );
        assert_eq!(
            canonical_path("/api/v1/user//create"),
            "/api/v1/user/create"
        );
        assert_eq!(
            canonical_path("/api/v1/user/%63reate"),
            "/api/v1/user/create"
        );
        assert_eq!(canonical_path("/"), "/");
        assert_eq!(canonical_path("//"), "/");
        // 非法转义：路由按字面量匹配，规范化不得意外改变语义
        assert_eq!(canonical_path("/api/v1%2"), "/api/v1%2");
    }

    /// 回归：已登记接口的路径变体（尾斜杠 / 重复斜杠 / percent 转义）
    /// 必须与规范路径一样被判为无权限，不得落回 fail-open 放行。
    #[tokio::test]
    async fn permission_denies_path_variants_of_registered_api() {
        let db = test_db().await;
        let user = seed_user(&db).await;
        let role = seed_role(&db).await;
        let api = seed_api(&db, &format!("/api/v1/{}/create", unique("apimw"))).await;
        sys_user_role::ActiveModel {
            user_id: Set(user.id),
            role_id: Set(role.id),
        }
        .insert(&db)
        .await
        .unwrap();
        // 角色只绑菜单、不绑该接口 → 所有写法都应被拒
        let state = app_state(db.clone()).await;

        // 先收集结果再断言：夹具清理必须发生在断言之前，
        // 否则断言 panic（如 RED 复现阶段）会把测试数据留在真库里。
        let mut outcomes = Vec::new();
        for variant in [
            api.path.clone(),
            format!("{}/", api.path),
            api.path.replace("/create", "//create"),
            api.path.replace("/create", "/%63reate"),
        ] {
            let (mut res, allowed) = run_permission(state.clone(), user.id, &variant, "POST").await;
            let rendered = !res.body_mut().is_none();
            outcomes.push((variant, allowed, rendered));
        }
        cleanup(&db, user.id, role.id, api.id).await;

        for (variant, allowed, rendered) in outcomes {
            assert!(
                !allowed,
                "路径变体 {variant} 不应放行（曾因原文查 sys_api 而 fail-open）"
            );
            assert!(rendered, "拒绝时必须渲染契约体 {variant}");
        }
    }

    /// 对照：角色真正绑定了该接口时，变体写法同样放行（规范化不能把合法请求挡掉）。
    #[tokio::test]
    async fn permission_allows_path_variant_when_role_is_authorized() {
        let db = test_db().await;
        let user = seed_user(&db).await;
        let role = seed_role(&db).await;
        let api = seed_api(&db, &format!("/api/v1/{}/create", unique("apimw"))).await;
        sys_user_role::ActiveModel {
            user_id: Set(user.id),
            role_id: Set(role.id),
        }
        .insert(&db)
        .await
        .unwrap();
        sys_role_api::ActiveModel {
            role_id: Set(role.id),
            api_id: Set(api.id),
        }
        .insert(&db)
        .await
        .unwrap();
        let state = app_state(db.clone()).await;

        let (_, allowed) = run_permission(state, user.id, &format!("{}/", api.path), "POST").await;
        // 断言前先清理，避免断言 panic 时留下夹具
        cleanup(&db, user.id, role.id, api.id).await;
        assert!(allowed, "已授权角色的尾斜杠写法应放行");
    }

    /// 未登记接口仍按既定语义放行（fail-open，渐进接管）。
    #[tokio::test]
    async fn permission_allows_unregistered_path_with_variant_spelling() {
        let db = test_db().await;
        let user = seed_user(&db).await;
        let state = app_state(db.clone()).await;

        let unregistered = format!("/api/v1/{}/nope/", unique("apimw"));
        let (_, allowed) = run_permission(state, user.id, &unregistered, "POST").await;

        sys_user::Entity::delete_by_id(user.id)
            .exec(&db)
            .await
            .unwrap();
        assert!(allowed, "未登记接口应放行（fail-open）");
    }
}
