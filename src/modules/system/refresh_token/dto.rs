//! 会话 DTO：entity 不直接暴露给接口，经 From 转换。

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_refresh_token;
use crate::utils::PageQuery;

/// 「在线」判定窗口：`last_active_at` 距今不超过该分钟数即视为在线。
pub const ONLINE_WINDOW_MINUTES: i64 = 5;

/// 会话响应体。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenResp {
    /// 会话 id
    pub id: u64,
    /// 用户 id
    pub user_id: u64,
    /// 登录用户名（冗余快照）
    pub username: String,
    /// 登录 IP
    pub ip: String,
    /// 登录 User-Agent
    pub agent: String,
    /// 登录时间（`yyyy-MM-dd HH:mm:ss`）
    pub created_at: String,
    /// 最后活跃时间（`yyyy-MM-dd HH:mm:ss`）
    pub last_active_at: String,
    /// 会话过期时间（`yyyy-MM-dd HH:mm:ss`）
    pub expires_at: String,
    /// 是否在线（`last_active_at` 在 [`ONLINE_WINDOW_MINUTES`] 分钟内）
    pub online: bool,
    /// 吊销时间；NULL 表示会话有效
    pub revoked_at: Option<String>,
    /// 吊销操作人（0=本人登出/系统，>0=管理员 user_id）
    pub revoked_by: u64,
    /// 吊销原因
    pub revoke_reason: String,
}

impl From<sys_refresh_token::Model> for RefreshTokenResp {
    fn from(m: sys_refresh_token::Model) -> Self {
        let now = chrono::Local::now().naive_local();
        Self {
            id: m.id,
            user_id: m.user_id,
            username: m.username,
            ip: m.ip,
            agent: m.agent,
            created_at: crate::utils::serde_format::format_datetime(m.created_at),
            last_active_at: crate::utils::serde_format::format_datetime(m.last_active_at),
            expires_at: crate::utils::serde_format::format_datetime(m.expires_at),
            online: now - m.last_active_at < chrono::Duration::minutes(ONLINE_WINDOW_MINUTES),
            revoked_at: m
                .revoked_at
                .map(crate::utils::serde_format::format_datetime),
            revoked_by: m.revoked_by,
            revoke_reason: m.revoke_reason,
        }
    }
}

/// 会话列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 用户名模糊搜索；不传查全部
    pub username: Option<String>,
    /// 仅看在线会话（`last_active_at` 在 [`ONLINE_WINDOW_MINUTES`] 分钟内）；不传查全部
    pub online_only: Option<bool>,
}

/// 会话分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct RefreshTokenFilter {
    pub username: Option<String>,
    pub online_only: Option<bool>,
}

/// 创建凭证记录入参（auth service 登录成功后调用；明文 token 由本域生成并只返回一次）。
pub struct CreateRefreshTokenParams {
    pub user_id: u64,
    pub username: String,
    pub ip: String,
    pub agent: String,
    /// 凭证有效期（秒），取 `config.jwt.refresh_ttl_seconds`
    pub refresh_ttl_seconds: i64,
}

/// 批量物理删除请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBatchReq {
    /// 要删除的记录 id 列表（仅死记录被删除；活跃记录与不存在的 id 静默跳过）
    pub ids: Vec<u64>,
}
