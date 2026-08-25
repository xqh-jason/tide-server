/// 业务错误类型。Handler 返回 `Result<T, AppError>`，由 Salvo 统一转成响应。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Biz(String),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl From<AppError> for salvo::Error {
    fn from(err: AppError) -> Self {
        salvo::Error::other(err)
    }
}
