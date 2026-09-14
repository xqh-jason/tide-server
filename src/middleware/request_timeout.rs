//! 请求级超时兜底中间件：把病态慢请求（DB 锁等待、下游阻塞等）在预算内结束掉。
//!
//! # 与响应契约的关系
//!
//! 超时渲染的是项目契约体（HTTP 200 + `{code:0,data:null,message}`），不产生 4xx/5xx，
//! 因此 `infra::catcher` 不会介入。没有直接用 salvo 自带的 `salvo_extra::timeout::Timeout`
//! 的原因：它只能渲染 `StatusError`（503 + 框架错误体），虽然本项目 catcher 会把 4xx/5xx
//! 洗成契约体，但文案会退化成通用「服务器内部错误」；自写可以给出准确的超时提示，
//! 且不引入 `salvo_extra` 依赖。结构沿用其成熟实现（`tokio::select!` + `Connection: close`
//! + `skip_rest`）。
//!
//! # 超时后的语义
//!
//! 超时即丢弃 handler future：`DatabaseTransaction` 随之 Drop 触发回滚（沿用本仓既有的事务
//! Drop 语义，见 menu 删除的超时兜底），连接归还连接池，单个请求无法长期独占连接。

use std::time::Duration;

use salvo::http::headers::{Connection, HeaderMapExt};
use salvo::prelude::*;

use crate::utils::response::ApiResponse;

/// 默认请求超时预算（秒）。
///
/// 取 30s 是为了兜住 DB 锁等待（`innodb_lock_wait_timeout` 默认 50s）与其它病态慢调用：
/// 真正的锁冲突已由 `utils::error` 把 1213/1205 映射成「操作冲突，请稍后重试」，本中间件
/// 兜的是「连错误都没返回」的长挂请求。
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

/// 超时提示文案（面向用户，`code: 0`）。
const TIMEOUT_MESSAGE: &str = "请求处理超时，请稍后重试";

/// 请求超时兜底中间件。
pub struct RequestTimeout {
    budget: Duration,
}

impl RequestTimeout {
    /// 以给定预算构造；预算内的请求不受影响。
    pub fn new(budget: Duration) -> Self {
        Self { budget }
    }
}

/// 是否豁免本请求的超时兜底。
///
/// 只豁免文件上传 / 下载两个**流式大对象**端点：单文件上限 10MB，慢网下传输时间天然可能
/// 超过预算，中途截断会留下半截响应/文件。其余端点（含 file 域的 list/get/delete 等 DB
/// 操作）都要兜底——DB 锁等待才是最需要被兜住的场景。
pub fn is_exempt(req: &Request) -> bool {
    let path = req.uri().path();
    path.ends_with("/file/upload") || path.ends_with("/file/download")
}

#[async_trait]
impl Handler for RequestTimeout {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        // 先取出请求标识：超时分支要写日志，而 call_next 已借走 req
        let method = req.method().as_str().to_string();
        let path = req.uri().path().to_string();

        tokio::select! {
            _ = ctrl.call_next(req, depot, res) => {}
            _ = tokio::time::sleep(self.budget) => {
                tracing::warn!(
                    method,
                    path,
                    timeout_secs = self.budget.as_secs(),
                    "请求处理超时，已放弃本次处理"
                );
                // 告知客户端连接即将关闭，避免其继续挂起等待（与 salvo 自带实现一致）
                res.headers_mut().typed_insert(Connection::close());
                res.render(Json(ApiResponse::<()>::fail(TIMEOUT_MESSAGE)));
                ctrl.skip_rest();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvo::test::{ResponseExt, TestClient};

    /// 慢 handler：睡 300ms 后正常返回（用于验证被超时兜底截断）。
    #[handler]
    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_millis(300)).await;
        "slow-done"
    }

    /// 快 handler：立即返回（用于验证正常请求不受影响）。
    #[handler]
    async fn fast() -> &'static str {
        "fast-done"
    }

    /// 预算 50ms，挂在与生产一致的 `hoop_when` 上（豁免判定一并生效）。
    fn service() -> Service {
        Service::new(
            Router::new()
                .hoop_when(RequestTimeout::new(Duration::from_millis(50)), |req, _| {
                    !is_exempt(req)
                })
                .push(Router::with_path("api/v1/slow").get(slow))
                .push(Router::with_path("api/v1/fast").get(fast))
                .push(Router::with_path("api/v1/file/upload").get(slow)),
        )
    }

    #[tokio::test]
    async fn slow_request_gets_timeout_body_with_http_200() {
        let mut res = TestClient::get("http://test/api/v1/slow")
            .send(&service())
            .await;

        let status = res.status_code.unwrap();
        let close = res
            .headers()
            .get("connection")
            .map(|v| v.to_str().unwrap().to_string());
        let body = res.take_string().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(status, StatusCode::OK, "超时也须 HTTP 200（项目契约）");
        assert_eq!(parsed["code"], 0, "超时应带 code=0");
        assert_eq!(parsed["message"], TIMEOUT_MESSAGE);
        assert!(parsed.get("data").is_some(), "契约体应包含 data 字段");
        assert_eq!(
            close.as_deref(),
            Some("close"),
            "超时应置 Connection: close"
        );
    }

    #[tokio::test]
    async fn fast_request_passes_through() {
        let mut res = TestClient::get("http://test/api/v1/fast")
            .send(&service())
            .await;

        assert_eq!(res.status_code.unwrap(), StatusCode::OK);
        assert_eq!(res.take_string().await.unwrap(), "fast-done");
    }

    #[tokio::test]
    async fn exempt_path_skips_timeout() {
        // 同一慢 handler 挂在豁免路径上：不被截断，正常返回
        let mut res = TestClient::get("http://test/api/v1/file/upload")
            .send(&service())
            .await;

        assert_eq!(res.status_code.unwrap(), StatusCode::OK);
        assert_eq!(
            res.take_string().await.unwrap(),
            "slow-done",
            "文件收发路径应豁免超时兜底"
        );
    }
}
