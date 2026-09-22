//! 加班域 DTO：请求 / 过滤条件 / 响应体声明。
//!
//! 约定同其它域：
//! - entity 不直接暴露给接口，响应体一律经 `From<Model>` 转换；
//! - 请求体**永不接受** `status`（状态机推进）、`duration_minutes`（后端按起止时间派生）、
//!   `*_by` / `*_name`（审计字段由 repo 盖章、人名字段由 service 与平台管道拼装）；
//! - `employee_name` 由 service 的 `fill_employee_names` 批量拼装，
//!   `created_by_name` / `updated_by_name` 由平台唯一管道 `utils::user_ref::fill_user_names`
//!   经 `UserRefNames` 填充（`From<Model>` 里一律留空串）；
//! - 时间字段一律 `String`（DTO 不做解析）：出参 `start_at` / `end_at` / `created_at` /
//!   `updated_at` 用 `yyyy-MM-dd HH:mm:ss`，`work_date` / 日期区间入参用 `yyyy-MM-dd`。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::hr_overtime_request;
use crate::utils::PageQuery;
use crate::utils::serde_format::format_datetime;
use crate::utils::user_ref::UserRefNames;

/// 日期格式化：`yyyy-MM-dd`（加班所属日期）。
fn fmt_date(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// 加班单列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OvertimeListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 员工档案 ID 精确过滤；不传查全部
    pub employee_id: Option<u64>,
    /// 状态精确过滤（1 审批中 2 已通过 3 已驳回 4 已撤销）；不传查全部
    pub status: Option<i8>,
    /// 加班类型精确过滤（1 工作日 2 休息日 3 法定节假日）；不传查全部
    pub overtime_type: Option<i8>,
    /// 加班日期范围起（`yyyy-MM-dd`，含边界）；不传不设下限
    pub work_date_begin: Option<String>,
    /// 加班日期范围止（`yyyy-MM-dd`，含边界）；不传不设上限
    pub work_date_end: Option<String>,
}

/// 加班单分页过滤条件（repo 层入参，分页参数另行传入）。
///
/// 日期区间在 service 层已解析为 `NaiveDate`（DTO 的 `String` 只走到 validate / service）。
#[derive(Debug, Clone, Default)]
pub struct OvertimeFilter {
    /// 员工档案 ID 精确过滤
    pub employee_id: Option<u64>,
    /// 状态精确过滤
    pub status: Option<i8>,
    /// 加班类型精确过滤
    pub overtime_type: Option<i8>,
    /// 加班日期范围起
    pub work_date_begin: Option<chrono::NaiveDate>,
    /// 加班日期范围止
    pub work_date_end: Option<chrono::NaiveDate>,
}

/// 创建加班单请求（建单即提交审批）。
///
/// **没有** `duration_minutes`：时长由后端按起止时间派生（`end_at - start_at` 的分钟数）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOvertimeReq {
    /// 员工档案 ID（hr_employee.id）
    pub employee_id: u64,
    /// 加班所属日期（`yyyy-MM-dd`；与起止时间同一天）
    pub work_date: String,
    /// 加班开始时间（`yyyy-MM-dd HH:mm:ss`）
    pub start_at: String,
    /// 加班结束时间（`yyyy-MM-dd HH:mm:ss`）
    pub end_at: String,
    /// 加班类型：1 工作日 2 休息日 3 法定节假日
    pub overtime_type: i8,
    /// 补偿方式：1 转调休 2 计加班费
    pub comp_mode: i8,
    /// 加班事由
    pub reason: String,
    /// 附件 ID（sys_file.id；0=无）
    #[serde(default)]
    pub attachment_id: u64,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 更新加班单请求（字段与创建一致 + 主键；仅已驳回 / 已撤销的单据可改）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOvertimeReq {
    /// 加班单主键
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 加班所属日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 加班开始时间（`yyyy-MM-dd HH:mm:ss`）
    pub start_at: String,
    /// 加班结束时间（`yyyy-MM-dd HH:mm:ss`）
    pub end_at: String,
    /// 加班类型：1 工作日 2 休息日 3 法定节假日
    pub overtime_type: i8,
    /// 补偿方式：1 转调休 2 计加班费
    pub comp_mode: i8,
    /// 加班事由
    pub reason: String,
    /// 附件 ID（sys_file.id；0=无）
    #[serde(default)]
    pub attachment_id: u64,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 我的加班单请求（申请人自助视角：只看当前用户档案下的单据）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MineReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 状态精确过滤（1 审批中 2 已通过 3 已驳回 4 已撤销）；不传查全部
    pub status: Option<i8>,
}

/// 加班单响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OvertimeResp {
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 加班所属日期（`yyyy-MM-dd`）
    pub work_date: String,
    /// 加班开始时间（`yyyy-MM-dd HH:mm:ss`）
    pub start_at: String,
    /// 加班结束时间（`yyyy-MM-dd HH:mm:ss`）
    pub end_at: String,
    /// 加班分钟数（后端派生：区间总长，不裁剪到班次窗口）
    pub duration_minutes: i32,
    /// 加班类型：1 工作日 2 休息日 3 法定节假日
    pub overtime_type: i8,
    /// 补偿方式：1 转调休 2 计加班费
    pub comp_mode: i8,
    /// 加班事由
    pub reason: String,
    /// 附件 ID（sys_file.id；0=无）
    pub attachment_id: u64,
    /// 状态：1 审批中 2 已通过 3 已驳回 4 已撤销
    pub status: i8,
    /// 审批实例 ID（hr_approval_instance.id；0=无）
    pub approval_instance_id: u64,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

/// `hr_overtime_request::Model` → `OvertimeResp` 字段搬运
/// （`employee_name` / 审计名字段留空，分别由 service 与 `fill_user_names` 拼装）。
impl From<hr_overtime_request::Model> for OvertimeResp {
    fn from(m: hr_overtime_request::Model) -> Self {
        Self {
            id: m.id,
            employee_id: m.employee_id,
            employee_name: String::new(),
            work_date: fmt_date(m.work_date),
            start_at: format_datetime(m.start_at),
            end_at: format_datetime(m.end_at),
            duration_minutes: m.duration_minutes,
            overtime_type: m.overtime_type,
            comp_mode: m.comp_mode,
            reason: m.reason,
            attachment_id: m.attachment_id,
            status: m.status,
            approval_instance_id: m.approval_instance_id,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for OvertimeResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}
