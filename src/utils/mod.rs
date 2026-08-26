pub mod error;
pub mod response;

pub use response::ApiResponse;

/// 统一 API 返回类型。handler 直接返回 `ApiResult<T>`：
/// - `Ok`：`ApiResponse<T>` → 自动渲染统一 JSON 响应体（code=200 契约）
/// - `Err`：`AppError` → 自动渲染 `{code, message}` 错误体
///
/// 比手写 `Result<Json<ApiResponse<T>>, AppError>` 简洁，且 `#[endpoint]` 契约不变。
pub type ApiResult<T> = Result<response::ApiResponse<T>, error::AppError>;
