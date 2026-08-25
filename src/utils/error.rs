//! 业务错误类型。Handler 返回 `Result<T, AppError>`，由 Salvo 的 `Writer` trait 统一渲染。
//!
//! Salvo 0.95 的错误处理机制：handler 返回 `Result<Ok, Err>` 要求：
//! - Ok 与 Err 都实现 `salvo::Writer`（运行时渲染）；
//! - `#[endpoint]` 额外要求 Err 实现 `salvo::oapi::EndpointOutRegister`（OpenAPI 文档化）。
//!
//! 因此为 `AppError` 实现 `Writer` + `EndpointOutRegister`，错误统一输出
//! `{code, data, message}` 契约体（与 vben successCode=200 对齐）。

use salvo::oapi::{self, EndpointOutRegister, ToSchema};
use salvo::prelude::*;

use crate::utils::response::ApiResponse;

/// 业务错误类型。Handler 返回 `Result<T, AppError>`，由 Salvo 的 `Writer` trait 统一渲染。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Biz(String),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// 让 AppError 成为合法的 handler 错误返回类型（运行时）。
/// Biz → HTTP 400 + 业务码 400；Internal → HTTP 500 + 业务码 500。
#[async_trait]
impl Writer for AppError {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        let (code, status, message) = match &self {
            AppError::Biz(msg) => (400, StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Internal(_) => {
                (500, StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        res.status_code(status);
        res.render(Json(ApiResponse::<()>::fail(code, message)));
    }
}

/// 让 AppError 的错误响应出现在 OpenAPI 文档中（#[endpoint] 要求）。
impl EndpointOutRegister for AppError {
    fn register(components: &mut oapi::Components, operation: &mut oapi::Operation) {
        for (code, desc) in [
            (StatusCode::BAD_REQUEST, "bad request"),
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        ] {
            operation.responses.insert(
                code.as_str(),
                oapi::Response::new(desc)
                    .add_content("application/json", ApiResponse::<()>::to_schema(components)),
            );
        }
    }
}
