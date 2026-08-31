pub mod cache;
pub mod crypt;
pub mod error;
pub mod jwt;
pub mod page;
pub mod response;

pub use page::{PageQuery, PageResult};
pub use response::ApiResponse;

/// 统一 API 返回类型。handler 直接返回 `ApiResult<T>`：
/// - `Ok`：`ApiResponse<T>` → 自动渲染统一 JSON 响应体（code=1 成功 / 0 失败契约）
/// - `Err`：`AppError` → 自动渲染 `{code, message}` 错误体
///
/// 比手写 `Result<Json<ApiResponse<T>>, AppError>` 简洁，且 `#[endpoint]` 契约不变。
pub type ApiResult<T> = Result<response::ApiResponse<T>, error::AppError>;
