//! sys_file_chunk 实体：断点续传分片记录（临时数据，硬删不软删）。
//!
//! 手写而非 codegen 产出：生成器四件套模板硬编码软删（deleted_at），
//! 本表无软删语义，且仅 5 列，手写成本低于"生成后裁剪"。
//! 唯一键 (file_md5, chunk_number) 由迁移保证（upsert 兜底约束）。

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "sys_file_chunk")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: u64,
    /// 整文件 md5（32 位小写 hex，断点会话标识）
    pub file_md5: String,
    /// 分片序号（0-based，< chunk_total）
    pub chunk_number: u32,
    /// 分片相对路径：chunks/<file_md5>/00042.part
    pub chunk_path: String,
    /// 分片写入时间（清理按此判过期）
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
