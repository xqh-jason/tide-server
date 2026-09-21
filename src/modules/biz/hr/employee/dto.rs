//! 员工档案 DTO：entity 不直接暴露给接口，经 From 转换；
//! 敏感字段（身份证 / 工资卡）在响应体脱敏，值域校验见同模块 `validate.rs`。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::hr_employee;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

/// 中段掩码：保留首 `keep_head` 与尾 `keep_tail` 个字符，其余替换为 `*`；
/// 长度不足（`len <= keep_head + keep_tail`）或空串时原样返回。
///
/// 按 `char` 处理而非字节，避免多字节字符被切断在 UTF-8 边界。
pub(crate) fn mask_middle(s: &str, keep_head: usize, keep_tail: usize) -> String {
    let chars = s.chars().collect::<Vec<char>>();
    let len = chars.len();

    if len <= keep_head.saturating_add(keep_tail) {
        return s.to_owned();
    }

    let mask_len = len - keep_head - keep_tail;
    let mut out = String::with_capacity(len);
    out.extend(chars.iter().take(keep_head).copied());
    out.push_str(&"*".repeat(mask_len));
    out.extend(chars.iter().skip(len - keep_tail).copied());

    out
}

/// 日期格式化：`Option<NaiveDate>` → `Option<String>`（`yyyy-MM-dd`）。
fn fmt_date(d: Option<chrono::NaiveDate>) -> Option<String> {
    d.map(|v| v.format("%Y-%m-%d").to_string())
}

/// 员工档案响应体（敏感字段脱敏后回传）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeResp {
    pub id: u64,
    /// 关联平台用户 ID
    pub user_id: u64,
    /// 关联账号显示名（username，后端批量拼装）
    pub user_name: String,
    /// 入职日期（`yyyy-MM-dd`）
    pub hire_date: Option<String>,
    /// 转正日期（`yyyy-MM-dd`）
    pub regular_date: Option<String>,
    /// 离职日期（`yyyy-MM-dd`）
    pub leave_date: Option<String>,
    /// 在职状态（字典 employmentStatus）
    pub employment_status: i8,
    /// 最高学历（字典 education）
    pub education: i8,
    pub graduate_school: String,
    pub major: String,
    /// 身份证号（脱敏：保留前 6 后 4）
    pub id_card: String,
    pub emergency_contact: String,
    pub emergency_phone: String,
    /// 工资卡号（脱敏：保留后 4）
    pub bank_account: String,
    pub remark: String,
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: u64,
    pub updated_by: u64,
    pub created_by_name: String,
    pub updated_by_name: String,
}

/// `hr_employee::Model` → `EmployeeResp` 字段搬运（敏感字段经 `mask_middle`）。
impl From<hr_employee::Model> for EmployeeResp {
    fn from(m: hr_employee::Model) -> Self {
        Self {
            id: m.id,
            user_id: m.user_id,
            user_name: String::new(),
            hire_date: fmt_date(m.hire_date),
            regular_date: fmt_date(m.regular_date),
            leave_date: fmt_date(m.leave_date),
            employment_status: m.employment_status,
            education: m.education,
            graduate_school: m.graduate_school,
            major: m.major,
            id_card: mask_middle(&m.id_card, 6, 4),
            emergency_contact: m.emergency_contact,
            emergency_phone: m.emergency_phone,
            bank_account: mask_middle(&m.bank_account, 0, 4),
            remark: m.remark,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        }
    }
}

/// 按名称映射填充创建人 / 更新人 / 关联账号显示名（查不到给空串）。
impl UserRefNames for EmployeeResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
        self.user_name = names.get(&self.user_id).cloned().unwrap_or_default();
    }
}

/// 员工档案列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeListReq {
    /// 分页参数（page / pageSize）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配备注 / 紧急联系人）；不传查全部
    pub keyword: Option<String>,
    /// 在职状态精确过滤；不传查全部
    pub employment_status: Option<i8>,
    /// 学历精确过滤；不传查全部
    pub education: Option<i8>,
    /// 创建人 ID 精确过滤；不传查全部
    pub created_by: Option<u64>,
    /// 更新人 ID 精确过滤；不传查全部
    pub updated_by: Option<u64>,
    /// 创建时间范围起（含边界）；不传查全部
    pub created_at_begin: Option<String>,
    /// 创建时间范围止（含边界）；不传查全部
    pub created_at_end: Option<String>,
    /// 更新时间范围起；不传查全部
    pub updated_at_begin: Option<String>,
    /// 更新时间范围止（含边界）；不传查全部
    pub updated_at_end: Option<String>,
}

/// 员工档案分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct EmployeeFilter {
    pub keyword: Option<String>,
    pub employment_status: Option<i8>,
    pub education: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 「同时创建登录账号」参数。
#[derive(Debug, Deserialize, ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountReq {
    /// 登录账号（全局唯一，含软删占位）
    pub username: String,
    /// 初始密码（服务端 Argon2id 哈希后落库）
    pub password: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 工号（空串表示未设置）
    #[serde(default)]
    pub emp_no: String,
    /// 手机号，空串表示未设置
    #[serde(default)]
    pub phone: String,
    /// 邮箱，空串表示未设置
    #[serde(default)]
    pub email: String,
    /// 角色 ID 列表，允许为空（不绑角色）
    #[serde(default)]
    pub role_ids: Vec<u64>,
}

/// 创建员工档案请求。
///
/// `user_id` 与 `create_account` **二选一**（恰好一个）：
/// - `user_id`：关联已存在的平台账号；
/// - `create_account`：在同一事务内新建账号后关联。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmployeeReq {
    /// 关联已有账号（与 createAccount 二选一）
    pub user_id: Option<u64>,
    /// 同时创建登录账号（与 userId 二选一）
    pub create_account: Option<CreateAccountReq>,
    /// 入职日期（`yyyy-MM-dd`）
    pub hire_date: Option<String>,
    /// 转正日期（`yyyy-MM-dd`）
    pub regular_date: Option<String>,
    /// 离职日期（`yyyy-MM-dd`）
    pub leave_date: Option<String>,
    /// 在职状态（字典 employmentStatus）
    pub employment_status: i8,
    /// 最高学历（字典 education）
    pub education: i8,
    /// 毕业院校
    #[serde(default)]
    pub graduate_school: String,
    /// 所学专业
    #[serde(default)]
    pub major: String,
    /// 身份证号
    #[serde(default)]
    pub id_card: String,
    /// 紧急联系人
    #[serde(default)]
    pub emergency_contact: String,
    /// 紧急联系人电话
    #[serde(default)]
    pub emergency_phone: String,
    /// 工资卡号
    #[serde(default)]
    pub bank_account: String,
    /// 备注
    #[serde(default)]
    pub remark: String,
}

/// 更新员工档案请求（不修改 user_id / 关联账号）。
///
/// 敏感字段（`id_card` / `bank_account`）**空串 = 不修改**：列表 / 详情回传的是掩码值，
/// 前端编辑表单不回填敏感字段（同 user 域密码的做法）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmployeeReq {
    /// 目标档案 id
    pub id: u64,
    /// 入职日期（`yyyy-MM-dd`）
    pub hire_date: Option<String>,
    /// 转正日期（`yyyy-MM-dd`）
    pub regular_date: Option<String>,
    /// 离职日期（`yyyy-MM-dd`）
    pub leave_date: Option<String>,
    /// 在职状态（字典 employmentStatus）
    pub employment_status: i8,
    /// 最高学历（字典 education）
    pub education: i8,
    #[serde(default)]
    pub graduate_school: String,
    #[serde(default)]
    pub major: String,
    /// 身份证号（空串 = 不修改）
    #[serde(default)]
    pub id_card: String,
    #[serde(default)]
    pub emergency_contact: String,
    #[serde(default)]
    pub emergency_phone: String,
    /// 工资卡号（空串 = 不修改）
    #[serde(default)]
    pub bank_account: String,
    #[serde(default)]
    pub remark: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> hr_employee::Model {
        hr_employee::Model {
            id: 9,
            user_id: 7,
            hire_date: None,
            regular_date: None,
            leave_date: None,
            employment_status: 1,
            education: 3,
            graduate_school: String::new(),
            major: String::new(),
            id_card: "110101199003071234".to_string(),
            emergency_contact: String::new(),
            emergency_phone: String::new(),
            bank_account: "6222021234567890123".to_string(),
            remark: String::new(),
            created_at: chrono::Local::now().naive_local(),
            updated_at: chrono::Local::now().naive_local(),
            created_by: 3,
            updated_by: 4,
            deleted_at: None,
        }
    }

    #[test]
    fn resp_masks_sensitive_fields() {
        let resp = EmployeeResp::from(model());
        assert_eq!(resp.id_card, "110101********1234");
        assert!(resp.bank_account.ends_with("0123"));
        assert!(!resp.bank_account.contains("6222"), "工资卡中段不得回传");
    }

    #[test]
    fn mask_middle_short_input_is_returned_as_is() {
        assert_eq!(mask_middle("1234", 6, 4), "1234");
        assert_eq!(mask_middle("", 6, 4), "");
    }
}
