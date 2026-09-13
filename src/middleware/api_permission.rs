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

use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::utils::response::ApiResponse;

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
        let path = req.uri().path().to_string();
        let method = req.method().as_str().to_string();

        match crate::modules::permission::service::has_api_permission(
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
