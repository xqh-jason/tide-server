//! job 域请求体校验：纯函数式校验（无第三方校验框架）。
//!
//! 结构约定：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, status_allowed) -> Result<(), String>`，命中规则即返回中文错误消息。
//! handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! cron / handler_name 的语义校验**委托 `scheduler`**（依赖任务注册表与 cron 解析器，
//! 属调度基础设施领域知识，不在此重复实现），错误经 `AppError` 的 Display 取文案。
//! status（启用/禁用）的允许值来自通用数据字典（`type="status"` 启用项，与用户/角色等
//! 模块一致；执行状态 `type="jobLogStatus"` 属 `sys_job_log`，不用于本域启停校验），
//! 由调用方预取后传入，通用判断见 `utils::check::check_status`。
//! service 层保留 scheduler 兜底校验（防非 HTTP 入口旁路）与查库查重。

use crate::modules::job::dto::{CreateJobReq, UpdateJobReq, UpdateJobStatusReq};
use crate::modules::job::scheduler;
use crate::utils::check;

/// job_name / cron_expr / handler_name 长度上限（对齐 `sys_job` 列定义 `VARCHAR(64)`）。
const NAME_MAX: usize = 64;
/// 备注长度上限（对齐 `sys_job.remark` `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;

/// 检查必填文本字段：trim 后非空，且不超过列长度。
fn check_required(s: &str, label: &str, errors: &mut Vec<String>) {
    if s.trim().is_empty() {
        errors.push(format!("{label}不能为空"));
    } else if s.chars().count() > NAME_MAX {
        errors.push(format!("{label}长度不能超过 {NAME_MAX} 个字符"));
    }
}

/// 收集到的错误拼接为一条消息（按字段检查顺序，可读性优于只给首条）。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 委托 scheduler 校验 handler_name / cron_expr，把错误文案收进 errors。
fn check_scheduler_fields(handler_name: &str, cron_expr: &str, errors: &mut Vec<String>) {
    if let Err(e) = scheduler::validate_handler_name(handler_name) {
        errors.push(e.to_string());
    }
    if let Err(e) = scheduler::validate_cron_expr(cron_expr) {
        errors.push(e.to_string());
    }
}

/// 任务写请求通用字段校验（create/update 共用，不含 id）。
fn check_common_fields(
    job_name: &str,
    cron_expr: &str,
    handler_name: &str,
    remark: &str,
    status: i8,
    status_allowed: &[i8],
    errors: &mut Vec<String>,
) {
    check_required(job_name, "任务名称", errors);
    check_required(cron_expr, "cron 表达式", errors);
    check_required(handler_name, "任务处理器名", errors);
    if remark.chars().count() > REMARK_MAX {
        errors.push(format!("备注长度不能超过 {REMARK_MAX} 个字符"));
    }
    check_scheduler_fields(handler_name, cron_expr, errors);
    check::check_status(status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
}

/// 创建定时任务请求校验。
pub fn validate_create_job(req: &CreateJobReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    check_common_fields(
        &req.job_name,
        &req.cron_expr,
        &req.handler_name,
        &req.remark,
        req.status,
        status_allowed,
        &mut errors,
    );
    join_errors(errors)
}

/// 更新定时任务请求校验。
pub fn validate_update_job(req: &UpdateJobReq, status_allowed: &[i8]) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("任务 ID 必须大于 0".to_string());
    }
    check_common_fields(
        &req.job_name,
        &req.cron_expr,
        &req.handler_name,
        &req.remark,
        req.status,
        status_allowed,
        &mut errors,
    );
    join_errors(errors)
}

/// 更新任务状态请求校验。
pub fn validate_update_job_status(
    req: &UpdateJobStatusReq,
    status_allowed: &[i8],
) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("任务 ID 必须大于 0".to_string());
    }
    check::check_status(req.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();
    join_errors(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_req() -> CreateJobReq {
        CreateJobReq {
            job_name: "测试任务".to_string(),
            cron_expr: "0 0 3 * * *".to_string(),
            handler_name: crate::task::login_log_cleanup::HANDLER_NAME.to_string(),
            status: 1,
            remark: String::new(),
        }
    }

    #[test]
    fn create_valid_request_passes() {
        assert!(validate_create_job(&create_req(), &[0, 1]).is_ok());
    }

    #[test]
    fn create_empty_job_name_reports_message() {
        let mut req = create_req();
        req.job_name = "  ".to_string();
        let err = validate_create_job(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("任务名称不能为空"), "实际: {err}");
    }

    #[test]
    fn create_unknown_handler_reports_message() {
        let mut req = create_req();
        req.handler_name = "no_such_handler".to_string();
        let err = validate_create_job(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("任务处理器不存在"), "实际: {err}");
    }

    #[test]
    fn create_invalid_cron_reports_message() {
        let mut req = create_req();
        req.cron_expr = "not-a-cron".to_string();
        let err = validate_create_job(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("cron 表达式不合法"), "实际: {err}");
    }

    #[test]
    fn create_invalid_status_reports_message() {
        let mut req = create_req();
        req.status = 9;
        let err = validate_create_job(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("状态取值不合法"), "实际: {err}");
    }

    #[test]
    fn update_zero_id_reports_message() {
        let req = create_req();
        let update = UpdateJobReq {
            id: 0,
            job_name: req.job_name,
            cron_expr: req.cron_expr,
            handler_name: req.handler_name,
            status: req.status,
            remark: req.remark,
        };
        let err = validate_update_job(&update, &[0, 1]).unwrap_err();
        assert!(err.contains("任务 ID 必须大于 0"), "实际: {err}");
    }

    #[test]
    fn update_job_status_zero_id_reports_message() {
        let req = UpdateJobStatusReq { id: 0, status: 1 };
        let err = validate_update_job_status(&req, &[0, 1]).unwrap_err();
        assert!(err.contains("任务 ID 必须大于 0"), "实际: {err}");
    }
}
