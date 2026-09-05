//! 文件 DTO：entity 不直接暴露给接口，经 From 转换。
//!
//! 规格依据：docs/superpowers/specs/2026-09-05-w5-file-upload-design.md §5。

use std::collections::HashMap;
use std::path::PathBuf;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::entity::sys_file;
use crate::utils::PageQuery;
use crate::utils::user_ref::UserRefNames;

/// 文件响应体（列表 / 详情项）。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileResp {
    /// 文件记录 id
    pub id: u64,
    /// 原始文件名（含扩展名，下载时用于 Content-Disposition）
    pub name: String,
    /// 磁盘存储名：`<uuid>.<ext>`（服务端生成，唯一）
    pub stored_name: String,
    /// 小写扩展名
    pub ext: String,
    /// Content-Type
    pub mime: String,
    /// 字节数
    pub size: u64,
    /// 上传时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub created_at: chrono::NaiveDateTime,
    /// 更新时间（`yyyy-MM-dd HH:mm:ss`）
    #[serde(serialize_with = "crate::utils::serde_format::naive_datetime")]
    pub updated_at: chrono::NaiveDateTime,
    /// 上传人 ID（`sys_user.id`）
    pub created_by: u64,
    /// 上传人显示名（`sys_user.username`，查不到给空串）
    pub created_by_name: String,
}

/// `sys_file::Model` → `FileResp` 字段搬运。
impl From<sys_file::Model> for FileResp {
    fn from(m: sys_file::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            stored_name: m.stored_name,
            ext: m.ext,
            mime: m.mime,
            size: m.size,
            created_at: m.created_at,
            updated_at: m.updated_at,
            created_by: m.created_by,
            created_by_name: String::new(),
        }
    }
}

/// 按名称映射填充 `FileResp` 的上传人显示名（查不到给空串）。
impl UserRefNames for FileResp {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
        self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
    }
}

/// 上传响应体：列表字段基础上增加 `url`（前端拼接站点域名后可直接使用）。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileUploadResp {
    #[serde(flatten)]
    pub file: FileResp,
    /// 下载地址：`/api/v1/file/download?id={id}`
    pub url: String,
}

/// 文件列表请求：分页字段内嵌 `PageQuery`，过滤条件在此声明。
#[derive(Debug, Deserialize, ToSchema)]
pub struct FileListReq {
    /// 分页参数（page / page_size）
    #[serde(flatten)]
    pub page: PageQuery,
    /// 原始文件名模糊搜索；不传查全部
    pub keyword: Option<String>,
}

/// 文件分页过滤条件（repo 层入参）。
#[derive(Debug, Clone, Default)]
pub struct FileFilter {
    pub keyword: Option<String>,
}

/// 上传业务入参（handler 从 multipart `file` 字段的 FilePart 组装）。
#[derive(Debug, Clone)]
pub struct UploadInput {
    /// 原始文件名（含扩展名；ext 由 service 从此推导并做白名单校验）
    pub name: String,
    /// Content-Type（`FilePart::content_type()`）
    pub mime: String,
    /// 字节数（`FilePart::size()`）
    pub size: u64,
    /// multipart 解析后 FilePart 的临时文件路径（作用域结束自动清理，落盘需 rename/copy）
    pub temp_path: PathBuf,
}
