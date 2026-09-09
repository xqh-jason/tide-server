//! CORS 防御中间件：按 `config.cors` 白名单控制跨源访问。
//!
//! 为什么不直接用 salvo 内置 `cors` feature：它依赖 `salvo-cors 0.95.2`，
//! rsproxy 镜像暂无该版本（与 `affix-state` 同因），故按项目惯例手写等价实现。
//!
//! 行为约定：
//! - 无 `Origin` 头的请求（同源 / curl 等非浏览器客户端）直接放行，不注入 CORS 头；
//! - `Origin` 不在 `allow_origins` 白名单：响应不含 CORS 头，浏览器拦截跨源读取；
//! - `Origin` 在白名单：回显 `Access-Control-Allow-Origin` 并追加 `Vary: Origin`；
//! - 预检请求（OPTIONS + `Access-Control-Request-Method`）：校验方法与请求头白名单，
//!   通过返回 204 + 完整 CORS 头并终结（避免 OPTIONS 落入业务路由 405），否则返回 403。
//!
//! 挂载位置：`Service` 层（包裹所有路由，含 swagger/openapi），
//! 由 app.rs 在 `InjectState`（router 层）之外持有配置副本构造，不依赖 Depot。

use salvo::http::{HeaderValue, Method, header};
use salvo::prelude::*;

use crate::infra::config::Cors as CorsConfig;

/// CORS 中间件：持有配置副本（`CorsConfig: Clone`）。
pub struct Cors(pub CorsConfig);

#[async_trait]
impl Handler for Cors {
    async fn handle(
        &self,
        req: &mut Request,
        _depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        // 非浏览器 / 同源请求无 Origin 头，直接放行。
        let Some(origin) = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
        else {
            return;
        };

        let allowed = self.0.allow_origins.iter().any(|o| o == origin);
        if allowed {
            let headers = res.headers_mut();
            if let Ok(value) = HeaderValue::from_str(origin) {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
            }
            headers.insert(header::VARY, HeaderValue::from_static("Origin"));
            if self.0.allow_credentials {
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                    HeaderValue::from_static("true"),
                );
            }
        }

        let is_preflight = req.method() == Method::OPTIONS
            && req
                .headers()
                .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
        if is_preflight {
            // 预检请求在此终结，不进入业务路由（业务层无 OPTIONS 端点，会 405）。
            if allowed && self.preflight_ok(req) {
                let headers = res.headers_mut();
                let methods = self.0.allow_methods.join(", ");
                let req_headers = self.0.allow_headers.join(", ");
                if let Ok(value) = HeaderValue::from_str(&methods) {
                    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, value);
                }
                if let Ok(value) = HeaderValue::from_str(&req_headers) {
                    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, value);
                }
                if let Ok(value) = HeaderValue::from_str(&self.0.max_age.to_string()) {
                    headers.insert(header::ACCESS_CONTROL_MAX_AGE, value);
                }
                res.status_code(StatusCode::NO_CONTENT);
            } else {
                res.status_code(StatusCode::FORBIDDEN);
            }
            ctrl.skip_rest();
        }
    }
}

impl Cors {
    /// 校验预检请求声明的方法与请求头是否都在白名单内。
    fn preflight_ok(&self, req: &Request) -> bool {
        let method_ok = req
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|v| v.to_str().ok())
            .map(|m| {
                self.0
                    .allow_methods
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(m))
            })
            .unwrap_or(false);

        // 无 Access-Control-Request-Headers 头 = 只请求简单头，视为通过。
        let headers_ok = req
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .map(|v| {
                v.to_str()
                    .map(|s| {
                        s.split(',').map(str::trim).all(|h| {
                            self.0
                                .allow_headers
                                .iter()
                                .any(|a| a.eq_ignore_ascii_case(h))
                        })
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(true);

        method_ok && headers_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvo::test::TestClient;

    fn config() -> CorsConfig {
        CorsConfig {
            allow_origins: vec!["http://localhost:5173".to_string()],
            allow_methods: vec!["GET".to_string(), "POST".to_string()],
            allow_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
            allow_credentials: false,
            max_age: 3600,
        }
    }

    #[handler]
    async fn hello() -> &'static str {
        "ok"
    }

    fn service() -> Service {
        let router = Router::new().get(hello).post(hello);
        Service::new(router).hoop(Cors(config()))
    }

    #[tokio::test]
    async fn same_origin_request_passes_through_without_cors_headers() {
        let res = TestClient::get("http://test/").send(&service()).await;

        assert_eq!(res.status_code, Some(StatusCode::OK));
        assert!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "同源请求不应注入 CORS 头"
        );
    }

    #[tokio::test]
    async fn allowed_origin_gets_allow_origin_header() {
        let res = TestClient::get("http://test/")
            .add_header(header::ORIGIN, "http://localhost:5173", true)
            .send(&service())
            .await;

        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:5173"),
            "白名单 origin 应回显 Access-Control-Allow-Origin"
        );
    }

    #[tokio::test]
    async fn disallowed_origin_gets_no_cors_header() {
        let res = TestClient::get("http://test/")
            .add_header(header::ORIGIN, "http://evil.com", true)
            .send(&service())
            .await;

        assert!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "非白名单 origin 的响应不应包含 CORS 头，浏览器会拦截"
        );
    }

    #[tokio::test]
    async fn preflight_from_allowed_origin_succeeds() {
        let res = TestClient::options("http://test/")
            .add_header(header::ORIGIN, "http://localhost:5173", true)
            .add_header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST", true)
            .add_header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type", true)
            .send(&service())
            .await;

        assert_eq!(res.status_code, Some(StatusCode::NO_CONTENT));
        assert!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .is_some(),
            "预检通过应返回允许的方法头"
        );
        assert!(
            res.headers().get(header::ACCESS_CONTROL_MAX_AGE).is_some(),
            "预检通过应返回缓存时长"
        );
    }

    #[tokio::test]
    async fn preflight_from_disallowed_origin_is_forbidden() {
        let res = TestClient::options("http://test/")
            .add_header(header::ORIGIN, "http://evil.com", true)
            .add_header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST", true)
            .send(&service())
            .await;

        assert_eq!(res.status_code, Some(StatusCode::FORBIDDEN));
    }

    #[tokio::test]
    async fn preflight_with_disallowed_method_is_forbidden() {
        let res = TestClient::options("http://test/")
            .add_header(header::ORIGIN, "http://localhost:5173", true)
            .add_header(header::ACCESS_CONTROL_REQUEST_METHOD, "DELETE", true)
            .send(&service())
            .await;

        assert_eq!(res.status_code, Some(StatusCode::FORBIDDEN));
    }

    #[tokio::test]
    async fn preflight_with_disallowed_header_is_forbidden() {
        let res = TestClient::options("http://test/")
            .add_header(header::ORIGIN, "http://localhost:5173", true)
            .add_header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST", true)
            .add_header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "x-custom-header",
                true,
            )
            .send(&service())
            .await;

        assert_eq!(res.status_code, Some(StatusCode::FORBIDDEN));
    }
}
