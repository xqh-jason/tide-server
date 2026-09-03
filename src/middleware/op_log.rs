//! 操作日志中间件（W5）：已登录业务请求自动落库（gin-vue-admin 对齐）。
//!
//! 本文件当前只含 AI 维护的失败测试；实现代码由业务作者补齐：
//! - `OperationLog`：中间件（AuthRequired 之后挂载）
//! - `sanitize_and_truncate`：JSON 递归脱敏 + 截断
//! - `truncate_utf8`：UTF-8 安全截断
//! - `utils::request::CapturedBody`：JsonBody 提取时写入 Depot 的原始请求体

use salvo::http::body::ResBody;
use salvo::prelude::*;
use sea_orm::ActiveValue::Set;

use crate::entity::sys_operation_log;
use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::utils::request::CapturedBody;

const MAX_BODY_BYTES: usize = 4096;
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "old_password",
    "new_password",
    "token",
    "authorization",
    "secret",
];
pub struct OperationLog;

#[async_trait]
impl Handler for OperationLog {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        let started = std::time::Instant::now();

        // 先拷贝请求元数据：call_next 需要 &mut req，之后不能再借用 req
        let path = req.uri().path().to_string();
        let method = req.method().as_str().to_string();
        let ip = req
            .remote_addr()
            .ip()
            .map(|addr| addr.to_string())
            .unwrap_or_default();
        let agent = req
            .headers()
            .get(salvo::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .chars()
            .take(255)
            .collect::<String>();
        let user_id = depot
            .get_typed::<AuthUser>()
            .map(|u| u.user_id)
            .unwrap_or(0);

        // 执行后续 handler（含真正的业务端点）
        ctrl.call_next(req, depot, res).await;

        // 响应体此时已写完：status 先读，body 取走记录后再放回
        let status = res.status_code.map(|c| c.as_u16() as i32).unwrap_or(200);
        let (resp_text, transport_error) = capture_response(res);
        // 业务失败（HTTP 200 + code:0）时，error_message 记业务 message；
        // 传输层错误（4xx/5xx 走 catcher）时记错误摘要
        let mut error_message = extract_biz_error_message(&resp_text);
        if error_message.is_empty() {
            error_message = transport_error;
        }

        // 请求体由 JsonBody 提取器在 call_next 内写入 Depot
        let body_text = depot
            .get_typed::<CapturedBody>()
            .map(|b| b.0.clone())
            .unwrap_or_default();

        let latency_ms = started.elapsed().as_millis() as i64;

        let Some(state) = depot.get_typed::<AppState>().ok() else {
            tracing::error!("op log: app state missing");
            return;
        };

        let model = sys_operation_log::ActiveModel {
            user_id: Set(user_id),
            ip: Set(ip),
            method: Set(method),
            path: Set(path),
            status: Set(status),
            latency: Set(latency_ms),
            agent: Set(agent),
            body: Set(sanitize_and_truncate(&body_text)),
            resp: Set(sanitize_and_truncate(&resp_text)),
            error_message: Set(truncate_utf8(&error_message, 500)),
            ..Default::default()
        };

        if let Err(err) =
            crate::modules::operation_log::repo::create_operation_log(&state.db, model).await
        {
            tracing::error!(%err, "operation log 落库失败");
        }
    }
}

/// 取出响应体文本并放回原 body；Error 变体只记录错误摘要。
fn capture_response(res: &mut Response) -> (String, String) {
    let saved = res.take_body();
    let (text, error_message) = match &saved {
        ResBody::Once(bytes) => (String::from_utf8_lossy(bytes).into_owned(), String::new()),
        ResBody::Chunks(chunks) => {
            let total_len: usize = chunks.iter().map(|c| c.len()).sum();
            let mut text = String::with_capacity(total_len);
            for chunk in chunks {
                text.push_str(&String::from_utf8_lossy(chunk));
            }
            (text, String::new())
        }
        ResBody::Error(status_error) => (
            String::new(),
            format!("响应未完成（HTTP 错误由 catcher 统一处理）: {status_error}"),
        ),
        _ => (String::new(), String::new()),
    };
    // 必须放回，否则客户端收不到响应
    *res.body_mut() = saved;
    (text, error_message)
}

/// 脱敏后截断：JSON 可解析时递归替换敏感键，非 JSON 原样截断。
fn sanitize_and_truncate(text: &str) -> String {
    let text = if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
        redact(&mut value);
        serde_json::to_string(&value).unwrap_or_else(|_| text.to_string())
    } else {
        text.to_string()
    };
    truncate_utf8(&text, MAX_BODY_BYTES)
}

/// 从统一契约体中提取业务失败 message（code = 0 时）。
fn extract_biz_error_message(resp_text: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(resp_text) else {
        return String::new();
    };
    if value.get("code").and_then(|c| c.as_i64()) == Some(0) {
        if let Some(message) = value.get("message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    String::new()
}

/// 递归把敏感键的值替换为 ***。
fn redact(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if SENSITIVE_KEYS.contains(&key.as_str()) {
                    *val = serde_json::Value::String("***".to_string());
                } else {
                    redact(val);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

/// UTF-8 安全截断：超长时落在字符边界并追加 ...(截断)。
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...(截断)", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_operation_log;
    use crate::infra::state::AppState;
    use crate::middleware::auth::AuthUser;
    use crate::utils::request::CapturedBody;
    use salvo::prelude::*;
    use sea_orm::{ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter};
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

    async fn cleanup(db: &DatabaseConnection, id: u64) {
        sys_operation_log::Entity::delete_by_id(id)
            .exec(db)
            .await
            .unwrap();
    }

    /// 脱敏：password / token / secret 递归替换为 ***，普通字段保留。
    #[tokio::test]
    async fn sanitize_json_text_redacts_sensitive_keys() {
        let input =
            r#"{"password":"pwd-1","nested":{"token":"tok-1"},"arr":[{"secret":"sec-1"}],"ok":1}"#;
        let out = sanitize_and_truncate(input);
        assert!(!out.contains("pwd-1"), "password 值应被脱敏");
        assert!(!out.contains("tok-1"), "token 值应被脱敏");
        assert!(!out.contains("sec-1"), "嵌套数组中的 secret 也应脱敏");
        assert!(out.contains("***"));
        assert!(out.contains("\"ok\":1"), "普通字段应原样保留");
    }

    /// 截断：落在 UTF-8 字符边界并带 ...(截断) 后缀。
    #[tokio::test]
    async fn truncate_utf8_keeps_char_boundary() {
        let text = "你".repeat(3000);
        let out = truncate_utf8(&text, 4096);
        assert!(out.ends_with("...(截断)"));
        assert!(out.len() <= 4096 + "...(截断)".len());
        assert!(text.starts_with(&out[..out.len() - "...(截断)".len()]));
    }

    /// 非 JSON 文本：不 panic，只做截断。
    #[tokio::test]
    async fn sanitize_non_json_falls_back_to_truncate() {
        let text = "plain ".repeat(1000);
        let out = sanitize_and_truncate(&text);
        assert!(out.ends_with("...(截断)"));
    }

    struct EchoHandler;

    #[async_trait]
    impl Handler for EchoHandler {
        async fn handle(
            &self,
            _req: &mut Request,
            _depot: &mut Depot,
            res: &mut Response,
            _ctrl: &mut FlowCtrl,
        ) {
            res.render(Json(crate::utils::ApiResponse::ok(serde_json::json!({
                "ok": true,
                "password": "should-be-redacted",
            }))));
        }
    }

    struct EchoFailureHandler;

    #[async_trait]
    impl Handler for EchoFailureHandler {
        async fn handle(
            &self,
            _req: &mut Request,
            _depot: &mut Depot,
            res: &mut Response,
            _ctrl: &mut FlowCtrl,
        ) {
            res.render(Json(crate::utils::ApiResponse::<serde_json::Value>::fail(
                "业务失败原因",
            )));
        }
    }

    /// 中间件自动落库：user_id / path / body / resp 记录正确且响应不被吞。
    #[tokio::test]
    async fn operation_log_middleware_persists_request_and_response() {
        let db = test_db().await;
        let config = crate::infra::config::Config::load().unwrap();
        let state = AppState::new(
            config,
            db.clone(),
            Arc::new(crate::utils::cache::MemoryCache::new()),
        );

        let path = format!("/api/v1/test/{}", unique("op"));
        let raw_body = r#"{"password":"pwd-mw","name":"alice"}"#;

        let mut req = Request::new();
        *req.uri_mut() = path.parse().unwrap();
        *req.method_mut() = salvo::http::Method::POST;

        let mut depot = Depot::new();
        depot.insert_typed(state);
        depot.insert_typed(AuthUser {
            user_id: 1,
            roles: vec![],
        });
        depot.insert_typed(CapturedBody(raw_body.to_string()));

        let mut res = Response::new();
        let mut ctrl = FlowCtrl::new(vec![Arc::new(OperationLog), Arc::new(EchoHandler)]);
        ctrl.call_next(&mut req, &mut depot, &mut res).await;

        let row = sys_operation_log::Entity::find()
            .filter(sys_operation_log::Column::Path.eq(path.clone()))
            .filter(sys_operation_log::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("中间件应写入操作日志");

        cleanup(&db, row.id).await;

        assert_eq!(row.user_id, 1, "操作人来自 AuthUser");
        assert_eq!(row.path, path);
        assert_eq!(row.method, "POST");
        assert!(row.body.contains("alice"), "请求体普通字段应入库");
        assert!(!row.body.contains("pwd-mw"), "请求体 password 应脱敏为 ***");
        assert!(row.resp.contains("\"ok\":true"), "响应体应被记录");
        assert!(
            !row.resp.contains("should-be-redacted"),
            "响应体敏感值也应脱敏"
        );
        assert!(!res.body.is_none(), "中间件不得吞掉业务响应");
    }

    /// 业务失败（code=0）时 error_message 记录业务 message，resp 存失败契约体。
    #[tokio::test]
    async fn operation_log_middleware_records_biz_error_message() {
        let db = test_db().await;
        let config = crate::infra::config::Config::load().unwrap();
        let state = AppState::new(
            config,
            db.clone(),
            Arc::new(crate::utils::cache::MemoryCache::new()),
        );

        let path = format!("/api/v1/test/{}", unique("fail"));
        let mut req = Request::new();
        *req.uri_mut() = path.parse().unwrap();
        *req.method_mut() = salvo::http::Method::POST;

        let mut depot = Depot::new();
        depot.insert_typed(state);
        depot.insert_typed(AuthUser {
            user_id: 2,
            roles: vec![],
        });
        depot.insert_typed(CapturedBody("{\"name\":\"bob\"}".to_string()));

        let mut res = Response::new();
        let mut ctrl = FlowCtrl::new(vec![Arc::new(OperationLog), Arc::new(EchoFailureHandler)]);
        ctrl.call_next(&mut req, &mut depot, &mut res).await;

        let row = sys_operation_log::Entity::find()
            .filter(sys_operation_log::Column::Path.eq(path.clone()))
            .filter(sys_operation_log::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .unwrap()
            .expect("业务失败也应写入操作日志");

        cleanup(&db, row.id).await;

        assert_eq!(row.error_message, "业务失败原因");
        assert!(row.resp.contains("业务失败原因"), "失败契约体应入库");
    }
}
