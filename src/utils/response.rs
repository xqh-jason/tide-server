use salvo::oapi::ToSchema;
use serde::Serialize;

/// 统一响应体：`{ code, data, message }`，`code = 200` 表示成功。
/// 与 vben v5 前端 request.ts 的 `successCode = 200` 契约对齐。
/// ToSchema 用于 #[endpoint] 生成 OpenAPI 文档。
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub data: T,
    pub message: String,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 200,
            data,
            message: "ok".to_string(),
        }
    }

    pub fn fail(code: i32, message: impl Into<String>) -> Self
    where
        T: Default,
    {
        Self {
            code,
            data: T::default(),
            message: message.into(),
        }
    }
}
