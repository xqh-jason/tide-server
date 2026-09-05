//! 业务错误类型。Handler 返回 `Result<T, AppError>`，由 Salvo 的 `Writer` trait 统一渲染。
//!
//! Salvo 0.95 的错误处理机制：handler 返回 `Result<Ok, Err>` 要求：
//! - Ok 与 Err 都实现 `salvo::Writer`（运行时渲染）；
//! - `#[endpoint]` 额外要求 Err 实现 `salvo::oapi::EndpointOutRegister`（OpenAPI 文档化）。
//!
//! 因此为 `AppError` 实现 `Writer` + `EndpointOutRegister`，错误统一输出
//! `{code, data, message}` 契约体（`code`: 1 成功 / 0 失败，与 vben successCode=1 对齐）。

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
/// 契约：业务/系统失败统一 HTTP 200 + `code: 0` + message（具体提示由 message 承担）；
/// Biz 返回业务消息，Internal 返回固定内部错误消息。
#[async_trait]
impl Writer for AppError {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        let message = match &self {
            AppError::Biz(msg) => msg.clone(),
            AppError::Internal(_) => "internal error".to_string(),
        };
        res.render(Json(ApiResponse::<()>::fail(message)));
    }
}

/// 让 AppError 不干扰 OpenAPI 文档中的成功响应（#[macro@endpoint] 要求实现）。
///
/// 关键点：`Result<ApiResponse<T>, AppError>` 的文档注册顺序是 Ok 先、Err 后，
/// 且两者都登记在 `"200"` key 下——若 Err 也 `insert` 会把成功响应覆盖，
/// 导致 Swagger 文档里看不到 `data` 字段的具体类型。
/// 运行时错误与成功同为 HTTP 200（body.code 区分 1/0），文档只为成功体建模，
/// 这里检测到已有 200 响应时直接跳过；仅当端点只可能返回错误时才登记错误体。
impl EndpointOutRegister for AppError {
    fn register(components: &mut oapi::Components, operation: &mut oapi::Operation) {
        if operation.responses.contains_key("200") {
            return;
        }
        operation.responses.insert(
            "200",
            oapi::Response::new("business error（HTTP 200，body.code=0）")
                .add_content("application/json", ApiResponse::<()>::to_schema(components)),
        );
    }
}
