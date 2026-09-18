//! 操作日志中间件：已登录业务请求自动落库。
//!
//! 组成：`OperationLog` 中间件（AuthRequired 之后挂载）、`sanitize_and_truncate`
//! JSON 递归脱敏 + 截断、`truncate_utf8` UTF-8 安全截断；
//! 原始请求体由 `utils::request::CapturedBody` 在 JsonBody 提取时写入 Depot。

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

/// 只读语义的路径后缀：这些端点不写操作日志。
///
/// 用后缀匹配是因为各域路径前缀不同（`/api/v1/user/list`、`/api/v1/role/list` ...）。
/// 操作日志的价值是「审计谁**改**了什么」，只读查询不改变状态，落库只是噪音 + 写压力。
const READ_ONLY_SUFFIXES: &[&str] = &[
    "/list",
    "/get",
    "/info",
    "/menus",
    "/access-codes",
    "/list-all",
    "/list-all-includes-soft-deleted",
    "/by-username",
    "/get-depts",
    "/get-positions",
    "/get-by-type",
    "/handlers",
    "/download",
];

/// 是否需要落操作日志：只记「可能改变状态」的请求。
///
/// 抽成纯函数（不碰 `Request` / `Depot`）是为了可单测——中间件本身难构造。
fn should_log(method: &str, path: &str) -> bool {
    // 非 POST 一律不记：本项目 GET 只有 file/download 与 site-config/get，均为只读；
    // 将来新增 GET 端点也天然被排除。
    if method != "POST" {
        return false;
    }
    // 去掉查询串后再判后缀（下载接口形如 /file/download?id=3）
    let path = path.split('?').next().unwrap_or(path);
    !READ_ONLY_SUFFIXES
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

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

        // 只读请求（list/get/info 等）与所有非 POST：放行业务后直接返回，不落日志。
        //
        // 写放大背景：本中间件挂在所有受保护路由上，若不做此判定，
        // 用户每翻一页（只读）也会产生一条 sys_operation_log，且该表被
        // 90 天清理任务物理删除，属无效占用。
        //
        // ⚠️ 关键：必须**先 call_next 放行业务**再 return。直接 return 会让请求
        // 得不到任何业务处理（客户端收到空响应）。
        if !should_log(&method, &path) {
            ctrl.call_next(req, depot, res).await;
            return;
        }
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
            crate::modules::system::operation_log::repo::create_operation_log(&state.db, model)
                .await
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
    if value.get("code").and_then(|c| c.as_i64()) == Some(0)
        && let Some(message) = value.get("message").and_then(|m| m.as_str())
    {
        return message.to_string();
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

    /// `should_log` 判定矩阵：只读端点/非 POST 不记，写端点要记。
    ///
    /// 这是写放大修复的回归守卫：若有人把只读端点加回记录范围，此用例会失败。
    #[test]
    fn should_log_excludes_read_only_and_non_post() {
        // —— 只读端点：不记 ——
        for path in [
            "/api/v1/user/list",
            "/api/v1/user/get",
            "/api/v1/user/info",
            "/api/v1/user/menus",
            "/api/v1/user/access-codes",
            "/api/v1/user/list-all",
            "/api/v1/user/list-all-includes-soft-deleted",
            "/api/v1/user/by-username",
            "/api/v1/user/get-depts",
            "/api/v1/user/get-positions",
            "/api/v1/dictionary/get-by-type",
            "/api/v1/job/handlers",
            "/api/v1/file/download?id=3",
        ] {
            assert!(!should_log("POST", path), "只读端点不应落操作日志：{path}");
        }

        // —— 写端点：要记 ——
        for path in [
            "/api/v1/user/create",
            "/api/v1/user/update",
            "/api/v1/user/update-status",
            "/api/v1/user/delete",
            "/api/v1/role/create",
            "/api/v1/file/upload",
        ] {
            assert!(should_log("POST", path), "写端点应落操作日志：{path}");
        }
    }

    /// 非 POST 一律不记；查询串不影响后缀判定。
    #[test]
    fn should_log_ignores_non_post_and_query_string() {
        assert!(!should_log("GET", "/api/v1/file/download"));
        assert!(!should_log("GET", "/api/v1/site-config/get"));
        assert!(!should_log("OPTIONS", "/api/v1/user/create"));
        assert!(!should_log("HEAD", "/api/v1/user/create"));
        assert!(
            should_log("POST", "/api/v1/user/create?x=1"),
            "写端点带查询串仍要记"
        );
    }

    /// ★ 关键回归：只读请求不落日志，**但必须仍然被业务处理**。
    ///
    /// 这是本修改最容易踩的坑：早退分支若漏了 `call_next`，
    /// 所有 list/get 请求会得到空响应（业务完全失效）。
    #[tokio::test]
    async fn read_only_request_skips_log_but_still_reaches_handler() {
        let db = test_db().await;
        let config = crate::infra::config::Config::load().unwrap();
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        let state = AppState::new(
            config,
            db.clone(),
            Arc::new(crate::utils::cache::MemoryCache::new()),
            scheduler,
        );

        // 路径必须以只读后缀结尾（should_log 按后缀判定）
        let path = format!("/api/v1/test/{}/list", unique("readonly"));
        let mut req = Request::new();
        *req.uri_mut() = path.parse().unwrap();
        *req.method_mut() = salvo::http::Method::POST;

        let mut depot = Depot::new();
        depot.insert_typed(state);
        depot.insert_typed(AuthUser {
            user_id: 1,
            roles: vec![],
            refresh_token_id: 0,
        });
        depot.insert_typed(CapturedBody("{\"page\":1}".to_string()));

        let mut res = Response::new();
        let mut ctrl = FlowCtrl::new(vec![Arc::new(OperationLog), Arc::new(EchoHandler)]);
        ctrl.call_next(&mut req, &mut depot, &mut res).await;

        // ① 业务必须被处理（本用例路径以 /list 结尾才能命中只读判定）
        let resp = capture_response(&mut res).0;
        assert!(
            resp.contains("\"ok\":true"),
            "只读请求必须仍然到达业务 handler，实际响应：{resp}"
        );

        // ② 不应落操作日志
        let row = sys_operation_log::Entity::find()
            .filter(sys_operation_log::Column::Path.eq(path.clone()))
            .one(&db)
            .await
            .unwrap();
        if let Some(ref row) = row {
            cleanup(&db, row.id).await;
        }
        assert!(row.is_none(), "只读端点不应写入操作日志：{path}");
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
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        let state = AppState::new(
            config,
            db.clone(),
            Arc::new(crate::utils::cache::MemoryCache::new()),
            scheduler,
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
            refresh_token_id: 0,
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
        let scheduler = Arc::new(tokio_cron_scheduler::JobScheduler::new().await.unwrap());
        let state = AppState::new(
            config,
            db.clone(),
            Arc::new(crate::utils::cache::MemoryCache::new()),
            scheduler,
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
            refresh_token_id: 0,
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
