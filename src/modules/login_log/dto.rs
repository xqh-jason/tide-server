//! 登录日志 DTO（codegen 生成）：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_login_log;
use crate::utils::PageQuery;

/// 登录日志响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginLogResp {
    /// 日志 id
    pub id: u64,
    /// 用户 id；未知用户（登录失败且查无此人）为 0
    pub user_id: u64,
    /// 尝试登录的用户名
    pub username: String,
    /// 客户端 IP
    pub ip: String,
    /// 浏览器 User-Agent
    pub agent: String,
    /// 登录结果：`1` 成功、`0` 失败
    pub status: i8,
    /// 结果说明（如「登录成功」「密码错误」「用户已被禁用」）
    pub msg: String,
    /// 记录时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    pub updated_at: String,
}

/// `sys_login_log::Model` → `LoginLogResp` 字段搬运。
impl From<sys_login_log::Model> for LoginLogResp {
    fn from(m: sys_login_log::Model) -> Self {
        Self {
            id: m.id,
            user_id: m.user_id,
            username: m.username,
            ip: m.ip,
            agent: m.agent,
            status: m.status,
            msg: m.msg,
            created_at: crate::utils::serde_format::format_datetime(m.created_at),
            updated_at: crate::utils::serde_format::format_datetime(m.updated_at),
        }
    }
}

/// 登录日志列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginLogListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 用户名模糊搜索；不传查全部
    pub username: Option<String>,
    /// IP 模糊搜索；不传查全部
    pub ip: Option<String>,
    /// 登录结果精确过滤：`1` 成功、`0` 失败；不传查全部
    pub status: Option<i8>,
}

/// 登录日志分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct LoginLogFilter {
    pub username: Option<String>,
    pub ip: Option<String>,
    pub status: Option<i8>,
}

/// 批量删除请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteBatchReq {
    /// 要删除的记录 id 列表（软删；不存在的 id 自动忽略）
    pub ids: Vec<u64>,
}
