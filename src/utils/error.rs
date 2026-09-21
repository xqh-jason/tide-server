//! 业务错误类型。Handler 返回 `Result<T, AppError>`，由 Salvo 的 `Writer` trait 统一渲染。
//!
//! Salvo 0.95 要求 `Result` 的 Ok/Err 都实现 `Writer`、`#[endpoint]` 的 Err 另需实现
//! `EndpointOutRegister`，因此本文件为 `AppError` 实现这两个 trait，错误统一输出
//! `{code, data, message}` 契约体（`code`: 1 成功 / 0 失败，与 vben successCode=1 对齐）。

use salvo::oapi;
use salvo::prelude::*;

use crate::utils::response::ApiResponse;

/// MySQL 死锁错误号（`Deadlock found when trying to get lock`）。
const MYSQL_ERR_DEADLOCK: u16 = 1213;
/// MySQL 锁等待超时错误号（`Lock wait timeout exceeded`）。
const MYSQL_ERR_LOCK_WAIT_TIMEOUT: u16 = 1205;
/// 锁冲突对调用方是「可重试」信息，与菜单删除超时同属 `Biz` 分工（面向用户的提示）。
const LOCK_CONFLICT_MESSAGE: &str = "操作冲突，请稍后重试";

/// 业务错误类型。Handler 返回 `Result<T, AppError>`，由 Salvo 的 `Writer` trait 统一渲染。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Biz(String),

    #[error("internal error: {0}")]
    Internal(#[source] anyhow::Error),
}

/// 集中把 anyhow 错误收进 `AppError`：`DbErr` 里的锁冲突（死锁 / 锁等待超时）转成可重试
/// 的业务文案，其余一律 `Internal`。
///
/// 手写而非 thiserror 的 `#[from]`：repo 层返回 `anyhow::Result` 并用 `?` 传播，拦截放在
/// 这里（唯一的收口点），既有的 `?` 与 `map_err(anyhow::Error::from)` 调用点全部自动受益，
/// 无需逐个改造。
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        if let Some(code) = mysql_lock_conflict_code(&err) {
            tracing::warn!(mysql_code = code, error = %err, "数据库锁冲突，提示调用方重试");
            return AppError::Biz(LOCK_CONFLICT_MESSAGE.to_string());
        }
        AppError::Internal(err)
    }
}

/// 从 anyhow 错误链里提取 MySQL 锁冲突错误号（1213 死锁 / 1205 锁等待超时）。
///
/// 用 `MySqlDatabaseError::number()`（错误号）而非 `code()`（SQLSTATE）：死锁与锁等待超时
/// 的 SQLSTATE 分别是 `40001` / `HY000`，区分不出两者，也无法与其它 `HY000` 错误区分。
/// 只识别这两个错误号，其余（唯一键冲突、连接异常等）返回 `None` 交给 `Internal`。
fn mysql_lock_conflict_code(err: &anyhow::Error) -> Option<u16> {
    let db_err = err
        .chain()
        .find_map(|e| e.downcast_ref::<sea_orm::DbErr>())?;
    // 死锁可能出现在 Exec/Query，也可能出现在 commit 的 Conn，三者都要覆盖
    let sqlx_err = match db_err {
        sea_orm::DbErr::Conn(sea_orm::RuntimeErr::SqlxError(e))
        | sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(e))
        | sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(e)) => e,
        _ => return None,
    };
    let sea_orm::SqlxError::Database(db) = sqlx_err else {
        return None;
    };
    let number = db.try_downcast_ref::<sea_orm::SqlxMySqlError>()?.number();
    matches!(number, MYSQL_ERR_DEADLOCK | MYSQL_ERR_LOCK_WAIT_TIMEOUT).then_some(number)
}

/// 让 AppError 成为合法的 handler 错误返回类型（运行时）。
/// 契约：业务/系统失败统一 HTTP 200 + `code: 0` + message（具体提示由 message 承担）；
/// Biz 返回业务消息，Internal 返回固定内部错误消息。
#[async_trait]
impl Writer for AppError {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        let message = match &self {
            AppError::Biz(msg) => msg.clone(),
            AppError::Internal(err) => {
                // 对外只回固定串（不泄漏 SQL / 连接串等内部细节），但底层错误必须落日志，
                // 否则这类错误既不可读也不可查
                tracing::error!(error = %err, "未处理的内部错误");
                "internal error".to_string()
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 普通 anyhow 错误保持 `Internal`，不被锁冲突分支误伤。
    #[test]
    fn from_plain_anyhow_error_is_internal() {
        let err = AppError::from(anyhow::anyhow!("boom"));
        assert!(
            matches!(err, AppError::Internal(_)),
            "普通错误应为 Internal: {err:?}"
        );
    }

    /// 非锁冲突的库错误（唯一键 / 连接异常等）保持 `Internal`——本次只识别 1213/1205，
    /// 不扩大范围（唯一键冲突的文案另行评估）。
    #[test]
    fn from_non_lock_db_error_is_internal() {
        let db_err = sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("bad sql".to_string()));
        let err = AppError::from(anyhow::Error::from(db_err));
        assert!(
            matches!(err, AppError::Internal(_)),
            "非锁冲突的库错误应为 Internal: {err:?}"
        );
    }

    /// 锁冲突的向下转型只认 MySQL 错误号；错误链里全是自己的错误时返回 `None`。
    #[test]
    fn mysql_lock_conflict_code_ignores_non_lock_errors() {
        assert!(
            mysql_lock_conflict_code(&anyhow::anyhow!("boom")).is_none(),
            "非库错误不应识别出锁冲突错误号"
        );
        let db_err = sea_orm::DbErr::RecordNotFound("menu".to_string());
        assert!(
            mysql_lock_conflict_code(&anyhow::Error::from(db_err)).is_none(),
            "非 SqlxError 变体不应识别出锁冲突错误号"
        );
    }
}
