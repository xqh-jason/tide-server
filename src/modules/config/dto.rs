//! 系统配置 DTO：分「键值参数（Param）」与「网站设置（Site）」两组。
//! entity 不直接暴露给接口，经 From 转换。

use std::collections::HashMap;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::{sys_config, sys_site_config};
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

// —— 键值参数 ——

/// 参数响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigResp {
    /// 参数 id
    pub id: u64,
    /// 参数名称（展示）
    pub config_name: String,
    /// 参数键（业务内唯一，含软删占位）
    pub config_key: String,
    /// 参数值
    pub config_value: String,
    /// 备注
    pub remark: String,
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
}

/// `sys_config::Model` → `ConfigResp` 字段搬运。
impl From<sys_config::Model> for ConfigResp {
    fn from(m: sys_config::Model) -> Self {
        Self {
            id: m.id,
            config_name: m.config_name,
            config_key: m.config_key,
            config_value: m.config_value,
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

/// 按名称映射填充 `ConfigResp` 的创建人/更新人显示名（查不到给空串）。
impl UserRefNames for ConfigResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
        self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
    }
}

/// 参数列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfigListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 模糊搜索关键字（匹配 config_name / config_key）；不传查全部
    pub keyword: Option<String>,
}

/// 参数分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct ConfigFilter {
    pub keyword: Option<String>,
}

/// 创建参数请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateConfigReq {
    /// 参数名称（展示）
    pub config_name: String,
    /// 参数键（业务内唯一，含软删占位）
    pub config_key: String,
    /// 参数值
    pub config_value: String,
    /// 备注，可空
    pub remark: Option<String>,
}

/// 更新参数请求（编辑表单全量提交）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateConfigReq {
    /// 目标参数 id
    pub id: u64,
    /// 参数名称（展示）
    pub config_name: String,
    /// 参数键（业务内唯一，排除自身查重）
    pub config_key: String,
    /// 参数值
    pub config_value: String,
    /// 备注，可空
    pub remark: Option<String>,
}

// —— 网站设置 ——

/// 网站设置更新请求（编辑表单全量提交，恒更新 id=1 行）。
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateSiteConfigReq {
    /// 站点名称
    pub name: String,
    /// logo 图片 URL（可来自 W5-4 文件上传）
    pub logo: String,
    /// 浏览器 tab 图标 URL
    pub ico: String,
    /// 水印文字
    pub watermark_text: String,
    /// 水印开关：`1` 开、`0` 关
    pub watermark_enable: i8,
    /// 水印类型：text / pic
    pub watermark_type: String,
    /// 水印图片 URL
    pub watermark_pic: String,
    /// 主题白黑：white / black
    pub mode: String,
    /// 侧边栏模式：dark / light / head
    pub side_mode: String,
    /// 主题色
    pub color: String,
}

/// 网站设置响应体（公开端点返回，不含审计人字段）。
#[derive(Debug, Serialize, ToSchema)]
pub struct SiteConfigResp {
    /// 恒为 1
    pub id: u64,
    /// 站点名称
    pub name: String,
    /// logo 图片 URL
    pub logo: String,
    /// 浏览器 tab 图标 URL
    pub ico: String,
    /// 水印文字
    pub watermark_text: String,
    /// 水印开关：`1` 开、`0` 关
    pub watermark_enable: i8,
    /// 水印类型：text / pic
    pub watermark_type: String,
    /// 水印图片 URL
    pub watermark_pic: String,
    /// 主题白黑：white / black
    pub mode: String,
    /// 侧边栏模式：dark / light / head
    pub side_mode: String,
    /// 主题色
    pub color: String,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
}

/// `sys_site_config::Model` → `SiteConfigResp` 字段搬运（公开端点不暴露审计人字段）。
impl From<sys_site_config::Model> for SiteConfigResp {
    fn from(m: sys_site_config::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            logo: m.logo,
            ico: m.ico,
            watermark_text: m.watermark_text,
            watermark_enable: m.watermark_enable,
            watermark_type: m.watermark_type,
            watermark_pic: m.watermark_pic,
            mode: m.mode,
            side_mode: m.side_mode,
            color: m.color,
            updated_at: m.updated_at,
        }
    }
}
