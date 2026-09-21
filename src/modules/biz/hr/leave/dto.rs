//! 假期额度域 DTO：列表请求 / repo 过滤条件 / 响应体声明。
//!
//! 约定：
//! - entity 不直接暴露给接口，响应体一律经 `From<Model>` 转换；
//! - 请求体永不接受 `*_by` / `*_name`（审计字段由 repo 盖章、人名字段由 service 拼装）；
//! - 时间字段一律 `String`（DTO 层不做解析，解析在 service）；
//! - `employee_name` / `leave_type_name` / `operator_name` 由 service 批量拼装后回填；
//!   `created_by_name` / `updated_by_name` 由平台唯一管道 `utils::user_ref::fill_user_names`
//!   经 `UserRefNames` 填充（`From<Model>` 里一律留空串）；
//! - quota 单位为分钟（`i32`），「天 ↔ 分钟」换算在前端按假别 `unit` / `min_unit_minutes` 完成。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{hr_leave_balance, hr_leave_balance_log, hr_leave_grant, hr_leave_type};
use crate::utils::PageQuery;
use crate::utils::serde_format::format_datetime;
use crate::utils::user_ref::UserRefNames;

/// 日期格式化：`yyyy-MM-dd`（额度批次的生效 / 失效日）。
fn fmt_date(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// 假期类型列表请求：分页字段内嵌 `PageQuery`，过滤条件只在此声明一次。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveTypeListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配类型编码 / 类型名称）；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤（字典：1 启用 0 停用）；不传查全部
    pub status: Option<i8>,
}

/// 假期类型分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct LeaveTypeFilter {
    /// 模糊搜索关键字（匹配类型编码 / 类型名称）
    pub keyword: Option<String>,
    /// 状态精确过滤（1 启用 0 停用）
    pub status: Option<i8>,
}

/// 创建假期类型请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateLeaveTypeReq {
    /// 类型编码（单列唯一，含软删占位）
    pub type_code: String,
    /// 类型名称
    pub type_name: String,
    /// 计量单位：1 天 2 小时
    pub unit: i8,
    /// 额度模式：1 扣额度 0 只记录不扣额度
    pub balance_mode: i8,
    /// 最小请假单位（分钟）：240=半天 480=一天
    pub min_unit_minutes: i32,
    /// 是否必须上传附件：1 是 0 否
    pub require_attachment: i8,
    /// 是否允许负余额：1 是 0 否
    pub allow_negative: i8,
    /// 计薪比例（千分比）：1000=全额 0=无薪
    pub pay_ratio: i32,
    /// 状态：1 启用 0 停用
    pub status: i8,
    /// 备注
    pub remark: String,
}

/// 更新假期类型请求（字段与创建一致 + 主键）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLeaveTypeReq {
    /// 假期类型主键
    pub id: u64,
    /// 类型编码（单列唯一，含软删占位）
    pub type_code: String,
    /// 类型名称
    pub type_name: String,
    /// 计量单位：1 天 2 小时
    pub unit: i8,
    /// 额度模式：1 扣额度 0 只记录不扣额度
    pub balance_mode: i8,
    /// 最小请假单位（分钟）：240=半天 480=一天
    pub min_unit_minutes: i32,
    /// 是否必须上传附件：1 是 0 否
    pub require_attachment: i8,
    /// 是否允许负余额：1 是 0 否
    pub allow_negative: i8,
    /// 计薪比例（千分比）：1000=全额 0=无薪
    pub pay_ratio: i32,
    /// 状态：1 启用 0 停用
    pub status: i8,
    /// 备注
    pub remark: String,
}

/// 假期类型响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveTypeResp {
    pub id: u64,
    /// 类型编码
    pub type_code: String,
    /// 类型名称
    pub type_name: String,
    /// 计量单位：1 天 2 小时
    pub unit: i8,
    /// 额度模式：1 扣额度 0 只记录不扣额度
    pub balance_mode: i8,
    /// 最小请假单位（分钟）
    pub min_unit_minutes: i32,
    /// 是否必须上传附件：1 是 0 否
    pub require_attachment: i8,
    /// 是否允许负余额：1 是 0 否
    pub allow_negative: i8,
    /// 计薪比例（千分比）
    pub pay_ratio: i32,
    /// 状态：1 启用 0 停用
    pub status: i8,
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

/// `hr_leave_type::Model` → `LeaveTypeResp` 字段搬运（人名字段留空待 `fill_user_names` 拼装）。
impl From<hr_leave_type::Model> for LeaveTypeResp {
    fn from(m: hr_leave_type::Model) -> Self {
        Self {
            id: m.id,
            type_code: m.type_code,
            type_name: m.type_name,
            unit: m.unit,
            balance_mode: m.balance_mode,
            min_unit_minutes: m.min_unit_minutes,
            require_attachment: m.require_attachment,
            allow_negative: m.allow_negative,
            pay_ratio: m.pay_ratio,
            status: m.status,
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
impl UserRefNames for LeaveTypeResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 额度批次列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveGrantListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 员工档案 ID 精确过滤；不传查全部
    pub employee_id: Option<u64>,
    /// 假期类型 ID 精确过滤；不传查全部
    pub leave_type_id: Option<u64>,
    /// 发放依据模糊搜索（字典 leaveGrantReason）；不传查全部
    pub reason: Option<String>,
    /// 归属周期精确过滤（如 2026）；不传查全部
    pub period: Option<String>,
    /// 状态精确过滤（1 有效 2 已用尽 3 已失效 4 已撤销）；不传查全部
    pub status: Option<i8>,
}

/// 额度批次分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct LeaveGrantFilter {
    /// 员工档案 ID 精确过滤
    pub employee_id: Option<u64>,
    /// 假期类型 ID 精确过滤
    pub leave_type_id: Option<u64>,
    /// 发放依据模糊搜索
    pub reason: Option<String>,
    /// 归属周期精确过滤
    pub period: Option<String>,
    /// 状态精确过滤
    pub status: Option<i8>,
}

/// 批量发放额度请求。
///
/// 发放范围三选一（`all` / `dept_id` / `employee_ids`），由 `validate.rs` 校验。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateGrantReq {
    /// 指定员工档案 ID 列表（范围之一）
    pub employee_ids: Vec<u64>,
    /// 指定部门 ID（范围之一）
    pub dept_id: Option<u64>,
    /// 是否全员发放（范围之一）
    pub all: bool,
    /// 假期类型 ID
    pub leave_type_id: u64,
    /// 每人发放分钟数（恒正）
    pub minutes: i32,
    /// 发放依据（字典 leaveGrantReason，幂等键之一）
    pub reason: String,
    /// 归属周期（幂等键之一，如 2026）
    pub period: String,
    /// 生效日期（`yyyy-MM-dd`）
    pub effective_at: String,
    /// 失效日期（`yyyy-MM-dd`）；不传=永久有效
    pub expire_at: Option<String>,
    /// 备注
    pub remark: String,
}

/// 批量发放额度结果：命中即跳过（幂等靠「员工 × 假别 × 依据 × 周期」），
/// `skipped_employee_ids` 供前端提示哪些人本周期已发放。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchCreateGrantResp {
    /// 新建批次的员工数
    pub created: u64,
    /// 因幂等键已存在而跳过的员工数
    pub skipped: u64,
    /// 被跳过的员工档案 ID 列表
    pub skipped_employee_ids: Vec<u64>,
}

/// 额度批次响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveGrantResp {
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 假期类型 ID
    pub leave_type_id: u64,
    /// 假期类型名称（service 批量拼装）
    pub leave_type_name: String,
    /// 来源：1 发放 2 手工调整 3 加班转调休
    pub source: i8,
    /// 发放依据
    pub reason: String,
    /// 归属周期
    pub period: String,
    /// 授予分钟数
    pub minutes: i32,
    /// 剩余可用分钟数
    pub remaining_minutes: i32,
    /// 生效日期（`yyyy-MM-dd`）
    pub effective_at: String,
    /// 失效日期（`yyyy-MM-dd`）；`None`=永久有效
    pub expire_at: Option<String>,
    /// 状态：1 有效 2 已用尽 3 已失效 4 已撤销
    pub status: i8,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 创建人 ID（sys_user.id）
    pub created_by: u64,
    /// 更新人 ID（sys_user.id）
    pub updated_by: u64,
    /// 创建人显示名（`fill_user_names` 批量拼装）
    pub created_by_name: String,
    /// 更新人显示名（`fill_user_names` 批量拼装）
    pub updated_by_name: String,
}

/// `hr_leave_grant::Model` → `LeaveGrantResp` 字段搬运（人名字段留空待拼装）。
impl From<hr_leave_grant::Model> for LeaveGrantResp {
    fn from(m: hr_leave_grant::Model) -> Self {
        Self {
            id: m.id,
            employee_id: m.employee_id,
            employee_name: String::new(),
            leave_type_id: m.leave_type_id,
            leave_type_name: String::new(),
            source: m.source,
            reason: m.reason,
            period: m.period,
            minutes: m.minutes,
            remaining_minutes: m.remaining_minutes,
            effective_at: fmt_date(m.effective_at),
            expire_at: m.expire_at.map(fmt_date),
            status: m.status,
            remark: m.remark,
            created_at: format_datetime(m.created_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for LeaveGrantResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 额度账户列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveBalanceListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 员工档案 ID 精确过滤；不传查全部
    pub employee_id: Option<u64>,
    /// 假期类型 ID 精确过滤；不传查全部
    pub leave_type_id: Option<u64>,
    /// 账期精确过滤（自然年，如 2026）；不传查全部
    pub period: Option<String>,
}

/// 额度账户分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct LeaveBalanceFilter {
    /// 员工档案 ID 精确过滤
    pub employee_id: Option<u64>,
    /// 假期类型 ID 精确过滤
    pub leave_type_id: Option<u64>,
    /// 账期精确过滤
    pub period: Option<String>,
}

/// 额度账户响应体（聚合展示行）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveBalanceResp {
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 假期类型 ID
    pub leave_type_id: u64,
    /// 假期类型名称（service 批量拼装）
    pub leave_type_name: String,
    /// 账期（自然年）
    pub period: String,
    /// 累计授予
    pub granted_minutes: i32,
    /// 累计实扣
    pub used_minutes: i32,
    /// 审批中预占
    pub locked_minutes: i32,
    /// 累计失效作废
    pub expired_minutes: i32,
    /// 手工调整净额（可负）
    pub adjust_minutes: i32,
    /// 可用分钟数 = 授予 + 调整 − 实扣 − 预占 − 失效（service 可覆盖，`From` 里先按此计算）
    pub available_minutes: i64,
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

/// `hr_leave_balance::Model` → `LeaveBalanceResp` 字段搬运（人名字段留空待拼装）。
impl From<hr_leave_balance::Model> for LeaveBalanceResp {
    fn from(m: hr_leave_balance::Model) -> Self {
        Self {
            id: m.id,
            employee_id: m.employee_id,
            employee_name: String::new(),
            leave_type_id: m.leave_type_id,
            leave_type_name: String::new(),
            period: m.period,
            granted_minutes: m.granted_minutes,
            used_minutes: m.used_minutes,
            locked_minutes: m.locked_minutes,
            expired_minutes: m.expired_minutes,
            adjust_minutes: m.adjust_minutes,
            available_minutes: i64::from(m.granted_minutes) + i64::from(m.adjust_minutes)
                - i64::from(m.used_minutes)
                - i64::from(m.locked_minutes)
                - i64::from(m.expired_minutes),
            updated_at: format_datetime(m.updated_at),
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人显示名（查不到给空串）。
impl UserRefNames for LeaveBalanceResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 额度流水列表请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveBalanceLogListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 员工档案 ID 精确过滤；不传查全部
    pub employee_id: Option<u64>,
    /// 假期类型 ID 精确过滤；不传查全部
    pub leave_type_id: Option<u64>,
    /// 业务类型精确过滤（1 授予 2 手工调整 3 请假预占 4 审批实扣 5 驳回释放 6 过期作废）
    pub biz_type: Option<i8>,
}

/// 额度流水分页过滤条件（repo 层入参，分页参数另行传入）。
#[derive(Debug, Clone, Default)]
pub struct LeaveBalanceLogFilter {
    /// 员工档案 ID 精确过滤
    pub employee_id: Option<u64>,
    /// 假期类型 ID 精确过滤
    pub leave_type_id: Option<u64>,
    /// 业务类型精确过滤
    pub biz_type: Option<i8>,
}

/// 额度流水响应体（append-only 对账凭据）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaveBalanceLogResp {
    pub id: u64,
    /// 员工档案 ID
    pub employee_id: u64,
    /// 员工姓名（service 批量拼装）
    pub employee_name: String,
    /// 假期类型 ID
    pub leave_type_id: u64,
    /// 假期类型名称（service 批量拼装）
    pub leave_type_name: String,
    /// 授予批次 ID；0=账户级操作
    pub grant_id: u64,
    /// 业务类型：1 授予 2 手工调整 3 请假预占 4 审批实扣 5 驳回释放 6 过期作废
    pub biz_type: i8,
    /// 变动分钟数（正加负减）
    pub delta_minutes: i32,
    /// 变动前可用余额
    pub before_minutes: i32,
    /// 变动后可用余额
    pub after_minutes: i32,
    /// 来源类型：0 无 1 系统任务 2 请假单 3 加班单 4 手工
    pub source_kind: i8,
    /// 来源单据 / 批次 ID
    pub source_id: u64,
    /// 操作人 ID（0=系统）
    pub operator_id: u64,
    /// 操作人显示名（`fill_user_names` 批量拼装）
    pub operator_name: String,
    /// 备注
    pub remark: String,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
}

/// `hr_leave_balance_log::Model` → `LeaveBalanceLogResp` 字段搬运
/// （人名字段留空待拼装；流水无 `updated_at`）。
impl From<hr_leave_balance_log::Model> for LeaveBalanceLogResp {
    fn from(m: hr_leave_balance_log::Model) -> Self {
        Self {
            id: m.id,
            employee_id: m.employee_id,
            employee_name: String::new(),
            leave_type_id: m.leave_type_id,
            leave_type_name: String::new(),
            grant_id: m.grant_id,
            biz_type: m.biz_type,
            delta_minutes: m.delta_minutes,
            before_minutes: m.before_minutes,
            after_minutes: m.after_minutes,
            source_kind: m.source_kind,
            source_id: m.source_id,
            operator_id: m.operator_id,
            operator_name: String::new(),
            remark: m.remark,
            created_at: format_datetime(m.created_at),
        }
    }
}

/// 按名称映射填充操作人显示名（查不到给空串）。
///
/// 流水表是 append-only：**没有** `created_by` / `updated_by` 审计人字段对，
/// 唯一的人字段是 `operator_id`（0=系统，查不到自然给空串）。
impl UserRefNames for LeaveBalanceLogResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.operator_name = names.get(&self.operator_id).cloned().unwrap_or_default();
    }
}
