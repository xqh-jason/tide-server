//! 认证域 handler。
//!
//! Cookie 约定（spec：2026-09-13-auth-refresh-session-design.md §2）：
//! refresh token 只存 HttpOnly Cookie，不进任何响应体；salvo cookie feature
//! 未启用（rsproxy 镜像缺版本先例），解析/拼装手写 header 字符串。

use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::system::auth::dto::{LoginMeta, LoginReq, LoginResp};
use crate::modules::system::auth::service as auth_service;
use crate::modules::system::refresh_token::service as refresh_token_service;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::{ApiResponse, ApiResult};

/// refresh token 的 Cookie 名（vben 上游 mock 同名约定；HttpOnly，前端不感知）。
const REFRESH_COOKIE: &str = "refreshToken";

/// 从 `Cookie` 请求头提取指定 cookie 值（纯函数便于单测）。
///
/// 只做 `name=value` 切分与 trim，不做 URL 解码——本域 cookie 值为
/// uuid simple 的 32 位 hex，不含保留字符。
fn cookie_value(
    cookie_header: Option<&salvo::http::header::HeaderValue>,
    name: &str,
) -> Option<String> {
    let raw = cookie_header?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k.trim() == name && !v.trim().is_empty()).then(|| v.trim().to_string())
    })
}

/// 从请求中提取 refreshToken cookie 值。
fn extract_refresh_cookie(req: &Request) -> Option<String> {
    cookie_value(
        req.headers().get(salvo::http::header::COOKIE),
        REFRESH_COOKIE,
    )
}

/// 拼 `Set-Cookie` 值：`Max-Age` 取 refresh token 有效期（秒）。
fn refresh_cookie_value(token: &str, max_age_seconds: i64) -> String {
    format!("{REFRESH_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}")
}

/// 清除 refreshToken cookie（登出时随响应下发过期指令）。
fn clear_refresh_cookie_value() -> String {
    format!("{REFRESH_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// 登录：校验用户名密码，签发 JWT。公开接口（不挂认证中间件）。
#[endpoint]
pub async fn login(
    depot: &mut Depot,
    req: &Request,
    res: &mut Response,
    body: JsonBody<LoginReq>,
) -> ApiResult<LoginResp> {
    let state = AppState::from_depot(depot)?;
    let meta = LoginMeta {
        ip: req
            .remote_addr()
            .ip()
            .map(|addr| addr.to_string())
            .unwrap_or_default(),
        agent: req
            .headers()
            .get(salvo::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string(),
    };
    let (resp, refresh_token) = auth_service::login(
        &state.db,
        &state.config.jwt,
        // 显式 as_ref：Arc<dyn Cache> → &dyn Cache，编译器不做跨智能指针自动强转
        state.cache.as_ref(),
        body.into_inner(),
        meta,
    )
    .await?;
    // refresh token 只进 HttpOnly Cookie（明文不进响应体）；空串 = 会话创建
    // 尚未接线（TDD 红态），此时不种 Cookie，保持旧行为
    if !refresh_token.is_empty()
        && let Ok(value) = salvo::http::header::HeaderValue::from_str(&refresh_cookie_value(
            &refresh_token,
            state.config.jwt.refresh_ttl_seconds,
        ))
    {
        res.headers_mut()
            .append(salvo::http::header::SET_COOKIE, value);
    }
    Ok(ApiResponse::ok(resp))
}

/// 刷新 access token：凭 HttpOnly Cookie 中的 refresh token 换发新 JWT。
///
/// 公开端点（凭 refresh token 本身鉴权）；响应体是**裸 token 字符串**
/// （前端 baseRequestClient 直取 `resp.data` 当新 token），失败返回
/// **真 HTTP 401**（契约例外，前端 authenticateResponseInterceptor 据此触发重新登录）。
#[endpoint]
pub async fn refresh(depot: &mut Depot, req: &Request, res: &mut Response) {
    let state = AppState::from_depot(depot);
    let refresh_token = extract_refresh_cookie(req);

    let result = match (state, refresh_token) {
        (Ok(state), Some(token)) => {
            auth_service::refresh(&state.db, &state.config.jwt, &token).await
        }
        (Err(err), _) => Err(err),
        (_, None) => Err(AppError::Biz("未登录或登录已过期".into())),
    };

    match result {
        Ok(token) => res.render(Json(token)),
        Err(err) => {
            // 直写 401：不能走 AppError（Writer 恒 200），401 是契约中唯一的
            // HTTP 状态码例外，前端据此走重新登录
            res.status_code(StatusCode::UNAUTHORIZED);
            let message = match &err {
                AppError::Biz(msg) => msg.clone(),
                _ => "登录已过期，请重新登录".to_string(),
            };
            res.render(Json(ApiResponse::<()>::fail(&message)));
        }
    }
}

/// 登出：吊销当前会话（`revoked_by=0`，reason=「用户登出」）+ 清除 refreshToken Cookie。
/// 挂在本路由上的 `AuthRequired` 已确认会话有效；黑名单已随会话表落地退役
/// （spec §4.3），吊销即时生效、重启不丢。
#[endpoint]
pub async fn logout(depot: &mut Depot, res: &mut Response) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let actor = AuthUser::from_depot(depot)?;
    if let Err(err) = refresh_token_service::force_logout_refresh_token(
        &state.db,
        actor.refresh_token_id,
        0,
        "用户登出",
    )
    .await
    {
        match err {
            // 记录缺失/已吊销：幂等放行（重复登出、竞态下线均不视为失败）
            AppError::Biz(msg) => tracing::debug!(msg, "logout 幂等跳过"),
            // DB 故障必须让调用方感知，否则 Cookie 清了而服务端凭证仍存活
            AppError::Internal(_) => return Err(err),
        }
    }
    if let Ok(value) = salvo::http::header::HeaderValue::from_str(&clear_refresh_cookie_value()) {
        res.headers_mut()
            .append(salvo::http::header::SET_COOKIE, value);
    }
    Ok(ApiResponse::ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(s: &str) -> salvo::http::header::HeaderValue {
        salvo::http::header::HeaderValue::from_str(s).unwrap()
    }

    #[test]
    fn cookie_value_extracts_named_cookie() {
        let h = header("other=1; refreshToken=abc123; foo=bar");
        assert_eq!(
            cookie_value(Some(&h), "refreshToken").as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn cookie_value_ignores_other_names_and_empty_values() {
        let h = header("refreshToken=; other=2");
        assert_eq!(cookie_value(Some(&h), "refreshToken"), None, "空值视为缺失");
        assert_eq!(cookie_value(Some(&h), "other").as_deref(), Some("2"));
        assert_eq!(cookie_value(Some(&h), "missing"), None);
    }

    #[test]
    fn cookie_value_tolerates_spaces_and_missing_header() {
        let h = header("  refreshToken = spaced token  ");
        assert_eq!(
            cookie_value(Some(&h), "refreshToken").as_deref(),
            Some("spaced token")
        );
        assert_eq!(cookie_value(None, "refreshToken"), None);
    }

    #[test]
    fn refresh_cookie_roundtrip_sets_httponly_attributes() {
        let set = refresh_cookie_value("tok", 604800);
        assert!(set.starts_with("refreshToken=tok;"));
        assert!(set.contains("Path=/;"));
        assert!(set.contains("HttpOnly;"));
        assert!(set.contains("SameSite=Lax;"));
        assert!(set.contains("Max-Age=604800"));

        let clear = clear_refresh_cookie_value();
        assert!(clear.contains("Max-Age=0"), "清除应携带过期指令");
        // 清除值回灌解析：cookie 名一致、值为空
        let h = header(&clear);
        assert_eq!(cookie_value(Some(&h), "refreshToken"), None);
    }
}
