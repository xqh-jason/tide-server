use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::utils::response::ApiResponse;

/// 健康检查，返回统一响应体结构，供前端联调验证契约：
/// `{ "code": 1, "data": "ok", "message": "ok" }`
#[endpoint]
pub async fn health() -> Json<ApiResponse<String>> {
    Json(ApiResponse::ok("ok".to_string()))
}
