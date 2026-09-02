//! 全局错误兜底：把框架级错误（请求体解析失败 / 404 / 405 / 5xx 等）统一渲染成
//! 项目契约体 `{ code, data, message }` + HTTP 200。
//!
//! 背景：业务错误（`AppError`）已实现为 HTTP 200 + `code: 0`；但 Salvo 的提取器
//! （如 `JsonBody` 反序列化失败）会产出框架级 `ParseError` → 默认渲染成 HTTP 400 +
//! `{"error":{...}}`，与契约不一致。此 catcher 作为最后一道防线，把这些统一口径。
//!
//! 注意：`AuthRequired` 中间件的 401（未登录/登录过期）不经过此处，保持 HTTP 401
//! 语义，便于前端 authenticationResponseInterceptor 登出。

use salvo::catcher::Catcher;
use salvo::prelude::*;

use crate::utils::request::ParamErrorDetail;
use crate::utils::response::ApiResponse;

/// 统一错误兜底 handler：将当前 4xx/5xx 响应改写为 HTTP 200 + 契约错误体。
#[handler]
pub async fn handle_error(
    _req: &mut Request,
    _depot: &mut Depot,
    res: &mut Response,
    ctrl: &mut FlowCtrl,
) {
    let message = extract_error_message(res);
    res.status_code(StatusCode::OK);
    res.render(Json(ApiResponse::<()>::fail(message)));
    ctrl.skip_rest();
}

/// 生成全局 catcher。挂在 `Service` 上，捕获所有未被 handler 处理的框架级错误。
pub fn build() -> Catcher {
    Catcher::new(handle_error)
}

/// 从响应当前的错误状态中提取适合展示给前端的消息（统一中文，避免框架英文文案）。
fn extract_error_message(res: &Response) -> String {
    use salvo::http::ResBody;

    // 优先：body 中携带 StatusError（解析失败等），按状态码映射为中文提示。
    if let ResBody::Error(err) = &res.body {
        // 自定义 JsonBody 提取器已把错误翻译成带字段的中文提示，原样透传。
        // 用 downcast_origin 而不是解析 cause 字符串：origin 是我们在提取器里
        // 自己放入的类型（ParamErrorDetail），可以精确还原，且不会误伤框架自身的错误。
        if let Some(detail) = err.downcast_origin::<ParamErrorDetail>() {
            return detail.message().to_string();
        }
        return friendly_message(err.code);
    }

    // 兜底：按 HTTP 状态码给通用文案。
    res.status_code
        .map(friendly_message)
        .unwrap_or_else(|| "服务器内部错误".to_string())
}

/// 把 HTTP 状态码映射为对前端友好的中文提示。
fn friendly_message(code: StatusCode) -> String {
    match code {
        StatusCode::BAD_REQUEST => "请求参数格式错误".to_string(),
        StatusCode::UNAUTHORIZED => "未登录或登录已过期".to_string(),
        StatusCode::FORBIDDEN => "没有访问权限".to_string(),
        StatusCode::NOT_FOUND => "接口不存在".to_string(),
        StatusCode::METHOD_NOT_ALLOWED => "请求方法不支持".to_string(),
        StatusCode::CONFLICT => "数据冲突".to_string(),
        StatusCode::TOO_MANY_REQUESTS => "请求过于频繁".to_string(),
        _ if code.is_server_error() => "服务器内部错误".to_string(),
        // 其余未映射的 4xx 保留原始状态码描述（仍是英文，但语义清晰）。
        _ => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvo::oapi::endpoint;
    use salvo::oapi::extract::JsonBody;
    use salvo::test::{ResponseExt, TestClient};
    use serde::Deserialize;

    // 模拟一个需要完整字段的 DTO：缺字段会触发框架级反序列化失败（与 user/update 场景一致）。
    #[derive(Debug, Deserialize, ToSchema)]
    #[allow(dead_code)] // 仅用于验证反序列化契约，字段不参与断言
    struct StrictReq {
        pub id: u64,
        pub username: String,
    }

    // 模拟带 AuthRequired 保护的 handler：直接在此简化（不做真实鉴权，只验证合同体改写）。
    #[endpoint]
    async fn simulate_update(depot: &mut Depot, body: JsonBody<StrictReq>) -> String {
        let _ = depot;
        let _ = body;
        "ok".to_string()
    }

    fn router() -> Router {
        Router::new().push(
            Router::with_path("api/v1/user")
                // 简化：不挂 AuthRequired，直接挂 handler 与一个解析失败的场景
                .push(Router::with_path("update").post(simulate_update)),
        )
    }

    #[tokio::test]
    async fn missing_fields_get_uniform_error_body_with_http_200() {
        let service = Service::new(router()).catcher(build());

        // 缺字段（StrictReq 必填 username/id 都在，但少一个 username 模拟失败）
        // 这里模拟：只传 id 缺 username → 反序列化失败
        let mut res = TestClient::post("http://test/api/v1/user/update")
            .json(&serde_json::json!({ "id": 1 }))
            .send(&service)
            .await;

        let status = res.status_code.unwrap();
        let body = res.take_string().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(status, StatusCode::OK, "框架级错误应统一为 HTTP 200");
        assert_eq!(parsed["code"], 0, "错误应带 code=0");
        assert_eq!(parsed["message"], "请求参数格式错误");
        assert!(parsed.get("data").is_some(), "契约体应包含 data 字段");
    }

    #[tokio::test]
    async fn unknown_route_gets_uniform_error_body_with_http_200() {
        let service = Service::new(router()).catcher(build());

        let mut res = TestClient::get("http://test/api/v1/nonexistent")
            .send(&service)
            .await;

        let status = res.status_code.unwrap();
        let body = res.take_string().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(status, StatusCode::OK, "404 也应统一为 HTTP 200");
        assert_eq!(parsed["code"], 0);
        assert_eq!(parsed["message"], "接口不存在");
    }
}
