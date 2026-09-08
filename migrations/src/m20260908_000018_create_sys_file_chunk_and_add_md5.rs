//! W6-3 迁移：sys_file_chunk 分片记录表 + sys_file 加 md5 列（秒传支撑）。
//!
//! 分片记录为临时数据：硬删不软删（无 deleted_at / 审计字段），到期由
//! chunk_cleanup 定时任务清理。唯一键 (file_md5, chunk_number) 是分片幂等
//! upsert 的兜底约束。sys_file.md5 合并成功后回填，秒传按其查询未删记录。

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum SysFileChunk {
    Table,
    Id,
    FileMd5,
    ChunkNumber,
    ChunkPath,
    CreatedAt,
}

#[derive(DeriveIden)]
enum SysFile {
    Table,
    Md5,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SysFileChunk::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SysFileChunk::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key()
                            .comment("主键"),
                    )
                    .col(
                        ColumnDef::new(SysFileChunk::FileMd5)
                            .string_len(32)
                            .not_null()
                            .comment("整文件 md5（32 位小写 hex，断点会话标识）"),
                    )
                    .col(
                        ColumnDef::new(SysFileChunk::ChunkNumber)
                            .unsigned()
                            .not_null()
                            .comment("分片序号（0-based，< chunk_total）"),
                    )
                    .col(
                        ColumnDef::new(SysFileChunk::ChunkPath)
                            .string_len(255)
                            .not_null()
                            .comment("分片相对路径：chunks/<file_md5>/00042.part"),
                    )
                    .col(
                        ColumnDef::new(SysFileChunk::CreatedAt)
                            .date_time()
                            .not_null()
                            .default(Expr::current_timestamp())
                            .comment("分片写入时间（清理按此判过期）"),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uk_sys_file_chunk_file_md5_chunk_number")
                    .unique()
                    .table(SysFileChunk::Table)
                    .col(SysFileChunk::FileMd5)
                    .col(SysFileChunk::ChunkNumber)
                    .to_owned(),
            )
            .await?;
        // sys_file 加 md5 列（NULL + 索引）：存量记录 NULL 不参与秒传
        manager
            .alter_table(
                Table::alter()
                    .table(SysFile::Table)
                    .add_column(
                        ColumnDef::new(SysFile::Md5)
                            .string_len(32)
                            .null()
                            .comment("整文件 md5（断点续传合并后回填；秒传查询依据）"),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sys_file_md5")
                    .table(SysFile::Table)
                    .col(SysFile::Md5)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_sys_file_md5")
                    .table(SysFile::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(SysFile::Table)
                    .drop_column(SysFile::Md5)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(SysFileChunk::Table).to_owned())
            .await
    }
}
