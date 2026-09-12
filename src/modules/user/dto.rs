//! 用户 DTO（传输对象）：entity（Model）不直接暴露给接口，经 From 转换脱敏。
//!
//! 校验约定：请求体的值域校验**不写在 DTO 文件里**，见同模块 `validate.rs` 中
//! 手写的 `impl Validate`（规则与错误文案按字段分组，字段在此保持纯声明）。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::entity::sys_user;
use crate::middleware::auth::AuthUser;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

/// 用户简要响应体：全量用户下拉 / 审计过滤选择器专用（含软删用户）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserBriefResp {
    /// 用户 id
    pub id: u64,
    /// 用户名（显示名）
    pub username: String,
    /// 是否已软删：true 时前端选择器建议置灰或加「已删除」徽标
    pub deleted: bool,
}

impl From<sys_user::Model> for UserBriefResp {
    fn from(m: sys_user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            deleted: m.deleted_at.is_some(),
        }
    }
}

/// 用户响应体（不含密码等敏感字段）。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserResp {
    /// 用户 id
    pub id: u64,
    /// 用户名（登录账号，全局唯一）
    pub username: String,
    /// 工号；员工编号，空字符串表示未设置。
    pub emp_no: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 手机号，空字符串表示未设置
    pub phone: String,
    /// 邮箱，空字符串表示未设置
    pub email: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 创建时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 创建人 ID（`sys_user.id`；`0` 表示种子/系统写入）
    pub created_by: u64,
    /// 更新人 ID（`sys_user.id`）
    pub updated_by: u64,
    /// 创建人显示名（`sys_user.username`）
    pub created_by_name: String,
    /// 更新人显示名（`sys_user.username`）
    pub updated_by_name: String,
    /// 角色 ID 列表（`sys_role.id`）
    pub role_ids: Vec<u64>,
    /// 部门列表
    pub depts: Vec<UserDeptResp>,
}

/// `sys_user::Model` → `UserResp` 字段搬运。
impl From<sys_user::Model> for UserResp {
    fn from(m: sys_user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            emp_no: m.emp_no,
            nickname: m.nickname,
            phone: m.phone,
            email: m.email,
            status: m.status,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
            role_ids: Vec::new(),
            depts: Vec::new(),
        }
    }
}

/// 按名称映射填充 `UserResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for UserResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 用户列表请求（JSON body 源，统一契约）。
/// 分页字段用 `#[serde(flatten)]` 内嵌 `PageQuery`（单点定义在 utils），
/// 域过滤条件只在这里声明；handler 用一个 `JsonBody<UserListReq>` 提取全部。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 用户名模糊搜索关键字；不传查全部
    pub keyword: Option<String>,
    /// 状态精确过滤：`1` 启用、`0` 禁用；不传查全部
    pub status: Option<i8>,
    /// 创建人 ID 精确过滤（前端用户选择器回填 id）；不传查全部
    pub created_by: Option<u64>,
    /// 更新人 ID 精确过滤；不传查全部
    pub updated_by: Option<u64>,
    /// 创建时间范围起（`yyyy-MM-dd[ HH:mm:ss]`，含边界）；不传查全部
    pub created_at_begin: Option<String>,
    /// 创建时间范围止（含边界）；不传查全部
    pub created_at_end: Option<String>,
    /// 更新时间范围起（同上格式）；不传查全部
    pub updated_at_begin: Option<String>,
    /// 更新时间范围止（含边界）；不传查全部
    pub updated_at_end: Option<String>,
}

/// 用户分页过滤条件（repo 层入参）：过滤字段与分页参数分离，
/// 以后加过滤条件只改这里，repo 签名与调用点不变。
#[derive(Debug, Clone, Default)]
pub struct UserFilter {
    pub keyword: Option<String>,
    pub status: Option<i8>,
    pub created_by: Option<u64>,
    pub updated_by: Option<u64>,
    pub created_at_begin: Option<chrono::NaiveDateTime>,
    pub created_at_end: Option<chrono::NaiveDateTime>,
    pub updated_at_begin: Option<chrono::NaiveDateTime>,
    pub updated_at_end: Option<chrono::NaiveDateTime>,
}

/// 按用户名查询请求（JSON body 源）：`{ "username": "..." }`。
/// 该接口用于「用户名是否可用」检查，查询目标可能是任意输入，故只做必填不做格式限制。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsernameReq {
    /// 要查询的用户名
    pub username: String,
}

/// 用户信息响应（契约 §3.2）：`{ userInfo, roles }`，vben 的 fetchUserInfo 消费。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInfoResp {
    /// 用户基本信息（不含密码）
    #[serde(rename = "userInfo")]
    pub user_info: UserResp,
    /// 当前用户角色键列表（如 `["super"]`）
    pub roles: Vec<String>,
}

impl UserInfoResp {
    /// 从用户 Model 与登录态组装用户信息响应（roles 取自 AuthUser）。
    pub fn from_model(user: sys_user::Model, auth: &AuthUser) -> Self {
        Self {
            user_info: UserResp::from(user),
            roles: auth.roles.clone(),
        }
    }
}

/// 创建用户请求：`{ username, password, nickname?, phone?, email?, status?, role_ids?: [] }`
///
/// 值域校验见同模块 `validate.rs` 中 `impl Validate for CreateUserReq`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserReq {
    /// 用户名（登录账号，全局唯一，含软删占位）
    pub username: String,
    /// 初始密码，服务端 Argon2id 哈希后落库
    pub password: String,
    /// 工号；员工编号，空串表示未设置；非空必须为 6 位数字
    pub emp_no: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 手机号，空串表示未设置
    #[serde(default)]
    pub phone: String,
    /// 邮箱，空串表示未设置
    #[serde(default)]
    pub email: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 角色 ID 列表，允许空（不绑角色）
    pub role_ids: Vec<u64>,
    pub depts: Vec<UserDeptReq>,
}

/// 更新用户请求。值域校验见同模块 `validate.rs` 中 `impl Validate for UpdateUserReq`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserReq {
    /// 目标用户 id
    pub id: u64,
    /// 用户名（全局唯一，排除自身查重；不能与内置 admin 相同）
    pub username: String,
    /// 密码；传空串表示不更新密码（沿用原密文），非空则须 6-32 个字符
    pub password: String,
    /// 工号；员工编号，空串表示未设置；非空必须为 6 位数字
    pub emp_no: String,
    /// 用户昵称（显示名）
    pub nickname: String,
    /// 手机号，空串表示未设置
    #[serde(default)]
    pub phone: String,
    /// 邮箱，空串表示未设置
    #[serde(default)]
    pub email: String,
    /// 状态：`1` 启用、`0` 禁用
    pub status: i8,
    /// 角色 ID 列表
    pub role_ids: Vec<u64>,
    pub depts: Vec<UserDeptReq>,
}

/// 更新用户状态
/// `{ id, status }`
///
/// 值域校验见同模块 `validate.rs` 中 `impl Validate for UpdateUserStatusReq`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserStatusReq {
    /// 目标用户 id（内置 admin 不允许修改）
    pub id: u64,
    /// 目标状态：`1` 启用、`0` 禁用
    pub status: i8,
}

/// 用户-部门挂载项（请求侧）：一行 = 一个部门的一次任职。
///
/// 三个维度各管一件事：挂载行数 = 任职部门数（可多个）；`is_primary` = 主要组织归属
/// （**全局至多一个**，`depts` 非空时须恰好一个，用于展示/默认值，不参与数据权限）；
/// `is_leader` = 该部门负责人（可多个，数据权限直控凭据）。
#[derive(Debug, Deserialize, ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserDeptReq {
    /// 部门 id（须 > 0 且存在未软删；停用部门允许挂载）
    pub dept_id: u64,
    /// 是否主部门：`1` 是 / `0` 否（`depts` 非空时恰好一个 `1`）
    pub is_primary: i8,
    /// 是否该部门负责人：`1` 是 / `0` 否（可多个）
    pub is_leader: i8,
}

/// 用户-部门挂载项（响应侧）：随 `UserResp.depts` 返回，`deptName` 由后端批量拼装。
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserDeptResp {
    /// 部门 id
    pub dept_id: u64,
    /// 部门名（后端批量拼装；部门已软删时为空串）
    pub dept_name: String,
    /// 是否主部门：`1` 是 / `0` 否（至多一个 `1`）
    pub is_primary: i8,
    /// 是否该部门负责人：`1` 是 / `0` 否（可多个）
    pub is_leader: i8,
}
