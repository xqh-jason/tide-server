//! 文件业务：上传校验链、uuid 命名落盘、软删 + 物理删除、下载路径解析。
//!
//! 磁盘操作放本层（业务规则），repo 只管库。规格依据：
//! docs/superpowers/specs/2026-09-05-w5-file-upload-design.md §6 / §7 / §8.2。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use dashmap::DashMap;
use sea_orm::ActiveValue::Set;
use sea_orm::ConnectionTrait;

use crate::entity::sys_file;
use crate::modules::file::dto::{
    ChunkMergeReq, ChunkStatusReq, ChunkUploadInput, FileFilter, FileListReq, UploadInput,
};
use crate::modules::file::repo as file_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 分页查询（keyword 与审计过滤透传 repo）。
pub async fn page_files(
    db: &impl ConnectionTrait,
    req: &FileListReq,
) -> Result<PageData<sys_file::Model>, AppError> {
    let models = file_repo::find_page(
        db,
        &FileFilter {
            keyword: req.keyword.clone(),
            created_by: req.created_by,
            updated_by: req.updated_by,
            created_at_begin: crate::utils::datetime::parse_datetime(
                "createdAtBegin",
                &req.created_at_begin,
                false,
            )?,
            created_at_end: crate::utils::datetime::parse_datetime(
                "createdAtEnd",
                &req.created_at_end,
                true,
            )?,
            updated_at_begin: crate::utils::datetime::parse_datetime(
                "updatedAtBegin",
                &req.updated_at_begin,
                false,
            )?,
            updated_at_end: crate::utils::datetime::parse_datetime(
                "updatedAtEnd",
                &req.updated_at_end,
                true,
            )?,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(models)
}

/// 上传：校验链 → uuid 命名 → 落盘 → 写记录。
///
/// 校验链（规格 §6）：空名 → Biz("未选择文件")；扩展名小写后不在 `allows`
/// → Biz("不支持的文件类型：{ext}")；size 超限 → Biz("文件大小超出限制：{size} 字节")。
/// 磁盘 IO 失败 → `AppError::Internal`（规格 §7）。
pub async fn upload_file(
    db: &impl ConnectionTrait,
    dir: &Path,
    max_size: u64,
    allows: &[String],
    actor_id: u64,
    input: UploadInput,
) -> Result<sys_file::Model, AppError> {
    // —— 校验链 1：存在性 ——
    // handler 侧 multipart 里没有 file 字段时拿不到原始名，name 会是空串；
    // 在 service 统一兜底，保证业务规则只有一处。
    if input.name.trim().is_empty() {
        return Err(AppError::Biz("未选择文件".to_string()));
    }

    // —— 校验链 2：扩展名白名单 ——
    // rsplit_once('.') 取最后一个点之后的部分：「报告.v2.PDF」→ "PDF"；
    // 无点的名字（如 README）返回 None， unwrap_or_default() 落到空串，
    // 空串不会匹配任何白名单项，同样被拦下，不用单独写一个分支。
    let ext = input
        .name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase()) // .PDF 与 .pdf 同等对待，ext 也以小写入库
        .unwrap_or_default();
    // eq_ignore_ascii_case：防御 allows 配置里混进大写（如 "TXT"）不至于误拒
    let ext_allowed = allows.iter().any(|a| a.eq_ignore_ascii_case(&ext));
    if !ext_allowed {
        return Err(AppError::Biz(format!("不支持的文件类型：{ext}")));
    }

    // —— 校验链 3：大小上限 ——
    // 消息带的是实际字节数（用户关心「我的文件多大」），不是上限值；
    // 上限值由前端提示与配置文档承载。
    if input.size > max_size {
        return Err(AppError::Biz(format!(
            "文件大小超出限制：{} 字节",
            input.size
        )));
    }

    // —— 生成磁盘存储名：uuid v4 的 32 位连续 hex + 校验过的扩展名 ——
    // 刻意不用原始名做磁盘名，两个原因：
    // 1. 防覆盖：两个用户传同名文件互不影响；
    // 2. 防路径穿越：原始名可能包含 ../ 或特殊字符，uuid 是纯 hex，天然安全。
    // 原始名只存进 sys_file 记录，下载时从记录取来拼 Content-Disposition。
    let stored_name = format!("{}.{}", uuid::Uuid::new_v4().simple(), ext);

    // 上传目录不做启动预建（规格 §6）：首次上传时兜底创建，目录不存在不算部署错误。
    // create_dir_all 对已存在目录是幂等的，每次调也无妨。
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("创建上传目录失败：{e}")))?;

    // —— 落盘：优先 rename ——
    // 同一盘符下 rename 是零拷贝的元数据操作，比 copy 快得多；
    // 但 rename 跨文件系统会失败（CrossesDevices），仅该错误退化为 copy + 删临时文件，
    // 其余错误直接返回内部错误。W6-3 分片落盘 / 合并复用同一 helper。
    let dest = dir.join(&stored_name);
    persist_temp(&input.temp_path, &dest).await?;

    // —— 写记录 ——
    // 顺序有讲究：磁盘文件先就位，再落库。反过来若 insert 失败，
    // 磁盘上会留下一条没有记录引用的孤儿文件（无法从列表管理到它）。
    //
    // ActiveModel 只 Set 业务字段；created_at / updated_at / deleted_at 保持
    // NotSet（Default::default()），交给数据库默认值——与迁移定义单一事实来源。
    // created_by / updated_by 不在这里写：审计字段由 repo::create_file 统一盖章，
    // service 只透传 actor_id（AGENTS.md 写入口径）。
    let record = sys_file::ActiveModel {
        name: Set(input.name),
        stored_name: Set(stored_name),
        ext: Set(ext),
        mime: Set(input.mime),
        size: Set(input.size),
        ..Default::default()
    };
    // repo 返回 anyhow::Error，`?` 借助 AppError 的 #[from] 自动升级为 Internal
    Ok(file_repo::create_file(db, record, actor_id).await?)
}

/// 把 multipart 临时文件转存到目标路径：优先 rename（同盘零拷贝），
/// 仅 CrossesDevices 降级为 copy + 删临时文件，其余错误直接 Internal。
/// W5 上传与 W6-3 分片保存 / 合并落盘共用；临时文件 salvo 请求结束自动清理，
/// copy 分支主动删只是让磁盘早一点释放。
async fn persist_temp(temp: &Path, dest: &Path) -> Result<(), AppError> {
    if let Err(rename_err) = tokio::fs::rename(temp, dest).await {
        tracing::debug!(error = %rename_err, "rename 落盘失败");
        if rename_err.kind() == ErrorKind::CrossesDevices {
            // 降级
            tokio::fs::copy(temp, dest)
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!("写入上传目录失败：{e}")))?;

            if let Err(e) = tokio::fs::remove_file(temp).await {
                tracing::warn!("删除临时文件失败：{}", e);
            }
        } else {
            return Err(AppError::Internal(anyhow::anyhow!(
                "rename 落盘失败：{rename_err}"
            )));
        }
    }
    Ok(())
}

// ── W6-3 断点续传：分片状态 / 保存 / 合并 / 放弃（规格 §5–§7） ──

/// 合并防重叠占坑：同一 md5 的 merge 互斥（W6-2 job 防重叠 dashmap entry 范式）。
static MERGE_LOCKS: OnceLock<DashMap<String, ()>> = OnceLock::new();

fn merge_locks() -> &'static DashMap<String, ()> {
    MERGE_LOCKS.get_or_init(Default::default)
}

/// 合并占坑 guard：Drop 自动释放，`?` 早退 / panic 均不漏。
struct MergeGuard(String);

impl Drop for MergeGuard {
    fn drop(&mut self) {
        merge_locks().remove(&self.0);
    }
}

/// 尝试占坑：已被占（同 md5 正在合并）返回 None。
fn acquire_merge_lock(file_md5: &str) -> Option<MergeGuard> {
    use dashmap::mapref::entry::Entry;
    match merge_locks().entry(file_md5.to_string()) {
        Entry::Occupied(_) => None,
        Entry::Vacant(v) => {
            v.insert(());
            Some(MergeGuard(file_md5.to_string()))
        }
    }
}

/// md5 格式校验：32 位小写 hex。
fn is_md5(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 断点探测：返回（已传分片序号, 秒传命中记录）。
///
/// 实现步骤：
/// 1. `is_md5(&req.file_md5)` 不通过 → Biz("fileMd5 格式不合法")；
/// 2. `file_repo::find_file_by_md5` 命中 → 秒传，返回 `(vec![], Some(model))`；
/// 3. 未命中 → `file_repo::find_chunk_numbers_by_md5`，返回 `(numbers, None)`。
pub async fn chunk_status(
    db: &impl ConnectionTrait,
    req: &ChunkStatusReq,
) -> Result<(Vec<u32>, Option<sys_file::Model>), AppError> {
    let _ = (db, req);
    todo!("W6-3 任务 5：断点探测")
}

/// 保存单个分片（幂等覆盖）。
///
/// 校验链（规格 §6.1，序固定）：
/// 1. `is_md5` → Biz("fileMd5 格式不合法")；
/// 2. `file_name` 空名 → Biz("未选择文件")；ext 白名单 → Biz("不支持的文件类型：{ext}")
///    （复用 W5 校验与文案）；
/// 3. `chunk_total` ∈ [1, 10_000] 且 `chunk_number < chunk_total`
///    → Biz("分片序号越界：{chunk_number}")；
/// 4. `input.size > chunk_max_bytes` → Biz("分片大小超出限制：{size} 字节")；
/// 5. `create_dir_all(dir/chunks/<file_md5>)` → 分片名 `format!("{:05}.part", chunk_number)`
///    → `persist_temp` 落盘 → `file_repo::upsert_chunk`（相对路径 `chunks/<md5>/<name>`）。
pub async fn save_chunk(
    db: &impl ConnectionTrait,
    dir: &Path,
    allows: &[String],
    chunk_max_bytes: u64,
    input: ChunkUploadInput,
) -> Result<(), AppError> {
    let _ = (db, dir, allows, chunk_max_bytes, input);
    todo!("W6-3 任务 5：分片保存")
}

/// 合并：校验 → 占坑 → 秒传短路 → 齐全校验 → 按序 append + 流式 md5 → 复核 →
/// 落正式文件 → 写 sys_file（md5 回填、created_by 盖章）→ 清分片。
///
/// 实现步骤（规格 §6.2，任何失败路径不得留下半成品）：
/// 1. 校验链：`is_md5` → 白名单（同 save_chunk 文案）→
///    `req.size > resumable_max_bytes` → Biz("文件大小超出限制：{size} 字节")；
/// 2. `acquire_merge_lock` 拿不到 → Biz("该文件正在合并中")；
/// 3. `find_file_by_md5` 命中 → 清分片（目录 + `delete_chunks_by_md5`）后 Ok(已有 model)；
/// 4. 齐全校验：`find_chunk_numbers_by_md5` 与 `[0, chunk_total)` 比对，
///    缺 → Biz("分片缺失：{n}")（n 取第一个缺失序号）；
/// 5. 按序 append 写 `dir/chunks/<md5>/merge_tmp`，逐块（8 KB buffer）流式喂 `md5::Context`；
/// 6. 复核：重算 hex ≠ file_md5 或实际字节数 ≠ req.size
///    → 删 merge_tmp → Biz("文件校验失败，请重新上传")；
/// 7. `persist_temp` rename 到 `dir/<uuid>.<ext>`；
/// 8. `file_repo::create_file` 写记录（name = file_name、ext 小写、
///    mime = req.mime.clone().unwrap_or("application/octet-stream")、
///    size = 实际字节数、md5 = Set(Some(file_md5))）；
/// 9. 清分片目录（remove_dir_all，缺失不报错）+ `delete_chunks_by_md5`
///    （失败仅 warn 日志，不影响主文件）。
pub async fn merge_chunks(
    db: &impl ConnectionTrait,
    dir: &Path,
    allows: &[String],
    resumable_max_bytes: u64,
    actor_id: u64,
    req: &ChunkMergeReq,
) -> Result<sys_file::Model, AppError> {
    let _ = (db, dir, allows, resumable_max_bytes, actor_id, req);
    todo!("W6-3 任务 5：分片合并")
}

/// 放弃上传：删分片目录（缺失不报错）+ 硬删记录，返回删除行数（幂等）。
///
/// 实现步骤：`tokio::fs::remove_dir_all(dir/chunks/<file_md5>)` 错误为 NotFound 则忽略、
/// 其余 Internal；`file_repo::delete_chunks_by_md5`。
pub async fn remove_chunks(
    db: &impl ConnectionTrait,
    dir: &Path,
    file_md5: &str,
) -> Result<u64, AppError> {
    let _ = (db, dir, file_md5);
    todo!("W6-3 任务 5：放弃上传")
}

/// 查询单个详情（不存在 → Biz("文件不存在：{id}")）。
pub async fn get_file(db: &impl ConnectionTrait, id: u64) -> Result<sys_file::Model, AppError> {
    if let Some(model) = file_repo::find_by_id(db, id).await? {
        Ok(model)
    } else {
        Err(AppError::Biz(format!("文件不存在：{id}")))
    }
}

/// 删除：判存在（不存在 → Biz）→ 软删记录 + 物理删除磁盘文件（`dir.join(stored_name)`）。
pub async fn delete_file(db: &impl ConnectionTrait, dir: &Path, id: u64) -> Result<(), AppError> {
    if let Some(model) = file_repo::find_by_id(db, id).await? {
        let path = dir.join(model.stored_name.clone());
        tokio::fs::try_exists(&path)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("文件不存在：{e}")))?;
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("删除文件失败：{e}")))?;
        file_repo::soft_delete_file(db, id).await?;
        Ok(())
    } else {
        Err(AppError::Biz(format!("文件不存在：{id}")))
    }
}

/// 下载解析：id → 记录 → `dir.join(stored_name)`，返回（记录，磁盘路径）。
/// 记录一并返回，供 handler 拼下载响应头（Content-Type 用 mime、
/// Content-Disposition 用原始名），避免同一 id 查两次库。
/// 不存在 → Biz("文件不存在：{id}")；记录在但磁盘文件缺失 → Biz("文件存储已丢失：{id}")。
/// 路径只由服务端 stored_name 拼装，禁止信任前端传参（防路径穿越）。
pub async fn download_path(
    db: &impl ConnectionTrait,
    dir: &Path,
    id: u64,
) -> Result<(sys_file::Model, PathBuf), AppError> {
    let Some(model) = file_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("文件不存在：{id}")));
    };
    let path = dir.join(&model.stored_name);
    // tokio 异步探测，避免同步 IO 阻塞 worker 线程
    if !tokio::fs::try_exists(&path)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("探测文件存在性失败：{e}")))?
    {
        return Err(AppError::Biz(format!("文件存储已丢失：{id}")));
    }
    Ok((model, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DatabaseConnection};
    use std::fs;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 默认白名单（与 config.toml `[upload].allows` 一致的子集即可）。
    fn allows() -> Vec<String> {
        ["png", "jpg", "pdf", "txt", "zip"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// 每个测试独立的临时上传目录（10 MB 默认上限），测后整体清理。
    fn tmp_dir(tag: &str) -> PathBuf {
        static ROOT: OnceLock<PathBuf> = OnceLock::new();
        let root = ROOT.get_or_init(|| {
            let p = std::env::temp_dir().join(format!("file_upload_test_{}", std::process::id()));
            fs::create_dir_all(&p).unwrap();
            p
        });
        let dir = root.join(format!("{tag}_{}", SEQ.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 在临时目录里造一个待上传的临时文件（模拟 FilePart 的 temp_path）。
    fn temp_file(dir: &Path, bytes: &[u8]) -> PathBuf {
        let p = dir.join(format!("tmp_{}", unique("src")));
        fs::write(&p, bytes).unwrap();
        p
    }

    fn input(name: &str, mime: &str, size: u64, temp_path: PathBuf) -> UploadInput {
        UploadInput {
            name: name.to_string(),
            mime: mime.to_string(),
            size,
            temp_path,
        }
    }

    #[tokio::test]
    async fn upload_file_rejects_unlisted_and_missing_extension() {
        let db = test_txn().await;
        let dir = tmp_dir("reject_ext");

        let bad_ext = upload_file(
            &db,
            &dir,
            10 * 1024 * 1024,
            &allows(),
            1,
            input(
                "a.exe",
                "application/x-msdownload",
                10,
                temp_file(&dir, b"x"),
            ),
        )
        .await;
        let no_ext = upload_file(
            &db,
            &dir,
            10 * 1024 * 1024,
            &allows(),
            1,
            input("README", "text/plain", 10, temp_file(&dir, b"x")),
        )
        .await;

        let bad_ext_msg = match bad_ext {
            Err(AppError::Biz(m)) => m,
            other => panic!("白名单外扩展名应返回 Biz，实际 {other:?}"),
        };
        assert!(
            bad_ext_msg.contains("不支持的文件类型"),
            "实际消息：{bad_ext_msg}"
        );
        let no_ext_msg = match no_ext {
            Err(AppError::Biz(m)) => m,
            other => panic!("无扩展名应返回 Biz，实际 {other:?}"),
        };
        assert!(
            no_ext_msg.contains("不支持的文件类型"),
            "实际消息：{no_ext_msg}"
        );
    }

    #[tokio::test]
    async fn upload_file_rejects_empty_name_and_oversize() {
        let db = test_txn().await;
        let dir = tmp_dir("reject_size");

        let no_file = upload_file(
            &db,
            &dir,
            10 * 1024 * 1024,
            &allows(),
            1,
            input("", "text/plain", 10, temp_file(&dir, b"x")),
        )
        .await;
        let oversize = upload_file(
            &db,
            &dir,
            100,
            &allows(),
            1,
            input("a.txt", "text/plain", 200, temp_file(&dir, b"x")),
        )
        .await;

        assert!(
            matches!(no_file, Err(AppError::Biz(ref m)) if m.contains("未选择文件")),
            "空文件名（无文件字段）应返回 Biz(未选择文件)，实际 {no_file:?}"
        );
        let oversize_msg = match oversize {
            Err(AppError::Biz(m)) => m,
            other => panic!("超大小应返回 Biz，实际 {other:?}"),
        };
        assert!(
            oversize_msg.contains("文件大小超出限制"),
            "实际消息：{oversize_msg}"
        );
        assert!(
            oversize_msg.contains("200"),
            "消息应带字节数：{oversize_msg}"
        );
    }

    #[tokio::test]
    async fn upload_file_stores_with_uuid_name_stamps_actor_and_persists_disk_file() {
        let db = test_txn().await;
        let dir = tmp_dir("upload_ok");
        let temp = temp_file(&dir, b"hello upload");

        let model = upload_file(
            &db,
            &dir,
            10 * 1024 * 1024,
            &allows(),
            42,
            input("报告.txt", "text/plain", 12, temp.clone()),
        )
        .await
        .unwrap();

        let disk_path = dir.join(&model.stored_name);
        let on_disk = fs::read(&disk_path);
        let by_id = file_repo::find_by_id(&db, model.id).await.unwrap();

        let _ = fs::remove_file(&temp);

        assert_eq!(model.name, "报告.txt", "原始名应原样入库");
        assert_eq!(model.ext, "txt", "扩展名应小写入库");
        assert_eq!(model.created_by, 42, "created_by 应为上传人");

        let (stem, ext) = model
            .stored_name
            .rsplit_once('.')
            .expect("存储名应形如 <uuid>.<ext>");
        assert_eq!(ext, "txt");
        let parsed = uuid::Uuid::parse_str(stem).expect("存储名主干应为合法 uuid");
        assert_eq!(parsed.get_version_num(), 4, "应为 uuid v4");
        assert_ne!(model.stored_name, "报告.txt", "存储名不得沿用原始名");

        assert!(
            on_disk.is_ok(),
            "落盘文件应存在且可读：{}",
            disk_path.display()
        );
        assert_eq!(
            on_disk.unwrap(),
            b"hello upload",
            "落盘内容应与临时文件一致"
        );
        assert!(by_id.is_some(), "记录应已写入");
    }

    #[tokio::test]
    async fn delete_file_soft_deletes_record_and_removes_disk_file() {
        let db = test_txn().await;
        let dir = tmp_dir("delete");
        let temp = temp_file(&dir, b"to be deleted");

        let model = upload_file(
            &db,
            &dir,
            10 * 1024 * 1024,
            &allows(),
            1,
            input("gone.txt", "text/plain", 13, temp.clone()),
        )
        .await
        .unwrap();
        let disk_path = dir.join(&model.stored_name);
        assert!(disk_path.exists(), "前置：上传后磁盘文件应存在");

        delete_file(&db, &dir, model.id).await.unwrap();
        let after = file_repo::find_by_id(&db, model.id).await.unwrap();
        let second = delete_file(&db, &dir, model.id).await;
        // 磁盘存在性断言必须在 cleanup 之前：cleanup 会整目录删除，放在后面就是空断言
        let disk_gone = !disk_path.exists();

        let _ = fs::remove_file(&temp);

        assert!(after.is_none(), "删除后记录不可见");
        assert!(disk_gone, "磁盘文件应被物理删除");
        assert!(
            matches!(second, Err(AppError::Biz(ref m)) if m.contains("文件不存在")),
            "重复删除应返回 Biz(文件不存在)，实际 {second:?}"
        );
    }

    #[tokio::test]
    async fn get_and_delete_missing_return_biz_error() {
        let db = test_txn().await;
        let dir = tmp_dir("missing");

        let get_missing = get_file(&db, 9_999_999_999).await;
        let delete_missing = delete_file(&db, &dir, 9_999_999_999).await;

        assert!(
            matches!(get_missing, Err(AppError::Biz(ref m)) if m.contains("文件不存在")),
            "实际 {get_missing:?}"
        );
        assert!(
            matches!(delete_missing, Err(AppError::Biz(ref m)) if m.contains("文件不存在")),
            "实际 {delete_missing:?}"
        );
    }

    #[tokio::test]
    async fn download_path_resolves_record_and_reports_lost_storage() {
        let db = test_txn().await;
        let dir = tmp_dir("download");
        let temp = temp_file(&dir, b"downloadable");

        let model = upload_file(
            &db,
            &dir,
            10 * 1024 * 1024,
            &allows(),
            1,
            input("dl.txt", "text/plain", 11, temp.clone()),
        )
        .await
        .unwrap();
        let disk_path = dir.join(&model.stored_name);

        let (ok_model, ok_path) = download_path(&db, &dir, model.id).await.unwrap();
        // 模拟异常删除：记录在、磁盘文件缺失
        fs::remove_file(&disk_path).unwrap();
        let lost = download_path(&db, &dir, model.id).await;
        let missing = download_path(&db, &dir, 9_999_999_999).await;

        let _ = fs::remove_file(&temp);

        assert_eq!(
            ok_model.id, model.id,
            "应同时带回记录（响应头要用 mime/原始名）"
        );
        assert_eq!(ok_path, disk_path, "应返回 stored_name 对应的磁盘路径");
        assert!(
            matches!(lost, Err(AppError::Biz(ref m)) if m.contains("文件存储已丢失")),
            "实际 {lost:?}"
        );
        assert!(
            matches!(missing, Err(AppError::Biz(ref m)) if m.contains("文件不存在")),
            "实际 {missing:?}"
        );
    }

    // ── W6-3 断点续传 service 测试（真库 + 独立临时分片目录；service 前缀数据防并发互扰） ──

    use crate::entity::sys_file_chunk;
    use crate::modules::file::dto::{ChunkMergeReq, ChunkStatusReq, ChunkUploadInput};
    use sea_orm::ActiveModelTrait;

    /// 真实 md5 hex（32 位小写）：声明值用它，保证 merge 成功链路可走通。
    fn chunk_md5(bytes: &[u8]) -> String {
        format!("{:x}", md5::compute(bytes))
    }

    fn chunk_input(
        file_md5: &str,
        file_name: &str,
        number: u32,
        total: u32,
        temp: PathBuf,
    ) -> ChunkUploadInput {
        ChunkUploadInput {
            file_md5: file_md5.to_string(),
            file_name: file_name.to_string(),
            chunk_number: number,
            chunk_total: total,
            size: std::fs::read(&temp).unwrap().len() as u64,
            temp_path: temp,
        }
    }

    fn merge_req(file_md5: &str, file_name: &str, total: u32, size: u64) -> ChunkMergeReq {
        ChunkMergeReq {
            file_md5: file_md5.to_string(),
            file_name: file_name.to_string(),
            chunk_total: total,
            size,
            mime: None,
        }
    }

    /// 直插一条带 md5 的文件记录（绕过校验链，纯秒传前置数据）。
    async fn seed_file_with_md5(db: &impl ConnectionTrait, file_md5: &str) -> sys_file::Model {
        let tag = chunk_md5(file_md5.as_bytes());
        sys_file::ActiveModel {
            name: Set(format!("秒传_{tag}.txt")),
            stored_name: Set(format!("{tag}.txt")),
            ext: Set("txt".to_string()),
            mime: Set("text/plain".to_string()),
            size: Set(1),
            md5: Set(Some(file_md5.to_string())),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn save_chunk_rejects_bad_md5_format() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_md5fmt");
        let temp = temp_file(&dir, b"x");

        let bad = save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input("xyz", "a.txt", 0, 3, temp),
        )
        .await;

        assert!(
            matches!(bad, Err(AppError::Biz(ref m)) if m.contains("fileMd5 格式不合法")),
            "实际 {bad:?}"
        );
    }

    #[tokio::test]
    async fn save_chunk_rejects_ext_outside_allowlist() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_ext");
        let temp = temp_file(&dir, b"x");

        let bad = save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&chunk_md5(b"x"), "a.exe", 0, 3, temp),
        )
        .await;

        assert!(
            matches!(bad, Err(AppError::Biz(ref m)) if m.contains("不支持的文件类型")),
            "实际 {bad:?}"
        );
    }

    #[tokio::test]
    async fn save_chunk_rejects_chunk_number_out_of_range() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_range");
        let md5 = chunk_md5(b"x");

        let over = save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 3, 3, temp_file(&dir, b"x")),
        )
        .await;
        let zero_total = save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 0, temp_file(&dir, b"x")),
        )
        .await;
        let huge_total = save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 10_001, temp_file(&dir, b"x")),
        )
        .await;

        for (label, r) in [
            ("number>=total", over),
            ("total=0", zero_total),
            ("total>10_000", huge_total),
        ] {
            assert!(
                matches!(r, Err(AppError::Biz(ref m)) if m.contains("分片序号越界")),
                "{label} 应返回 Biz(分片序号越界)，实际 {r:?}"
            );
        }
    }

    #[tokio::test]
    async fn save_chunk_rejects_oversize_chunk() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_size");
        let bytes = vec![0u8; 20];

        let bad = save_chunk(
            &db,
            &dir,
            &allows(),
            10,
            chunk_input(&chunk_md5(&bytes), "a.txt", 0, 3, temp_file(&dir, &bytes)),
        )
        .await;

        match bad {
            Err(AppError::Biz(m)) => {
                assert!(m.contains("分片大小超出限制"), "实际消息：{m}");
                assert!(m.contains("20"), "消息应带字节数：{m}");
            }
            other => panic!("超限分片应返回 Biz，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn save_chunk_overwrites_same_number_idempotently() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_overwrite");
        let bytes = b"hello chunk";
        let md5 = chunk_md5(bytes);

        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 2, temp_file(&dir, bytes)),
        )
        .await
        .unwrap();
        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 2, temp_file(&dir, b"retry bytes!")),
        )
        .await
        .unwrap();

        let numbers = file_repo::find_chunk_numbers_by_md5(&db, &md5)
            .await
            .unwrap();
        let on_disk = std::fs::read(dir.join("chunks").join(&md5).join("00000.part")).unwrap();

        assert_eq!(numbers, vec![0], "同 (md5, number) 重传记录不翻倍");
        assert_eq!(on_disk, b"retry bytes!", "重传应覆盖分片文件内容");
    }

    #[tokio::test]
    async fn chunk_status_lists_uploaded_and_hits_instant_upload() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_status");
        let bytes = b"status probe";
        let md5 = chunk_md5(bytes);

        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 3, temp_file(&dir, bytes)),
        )
        .await
        .unwrap();
        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 1, 3, temp_file(&dir, bytes)),
        )
        .await
        .unwrap();

        let req = ChunkStatusReq {
            file_md5: md5.clone(),
            file_name: "a.txt".to_string(),
        };
        let (uploaded, hit) = chunk_status(&db, &req).await.unwrap();
        assert_eq!(uploaded, vec![0, 1], "已传序号升序返回");
        assert!(hit.is_none(), "未合并前不秒传");

        let seeded = seed_file_with_md5(&db, &md5).await;
        let (uploaded2, hit2) = chunk_status(&db, &req).await.unwrap();
        assert_eq!(uploaded2, Vec::<u32>::new(), "秒传命中后不再关心分片");
        assert_eq!(
            hit2.map(|m| m.id),
            Some(seeded.id),
            "done=true 带回文件记录"
        );
    }

    #[tokio::test]
    async fn merge_chunks_rejects_missing_chunks() {
        let db = test_txn().await;
        let dir = tmp_dir("merge_missing");
        let bytes = b"part0 only";
        let md5 = chunk_md5(bytes);

        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 3, temp_file(&dir, bytes)),
        )
        .await
        .unwrap();

        let bad = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            1,
            &merge_req(&md5, "a.txt", 3, bytes.len() as u64),
        )
        .await;

        assert!(
            matches!(bad, Err(AppError::Biz(ref m)) if m.contains("分片缺失")),
            "实际 {bad:?}"
        );
    }

    #[tokio::test]
    async fn merge_chunks_rejects_md5_mismatch_and_cleans_tmp() {
        let db = test_txn().await;
        let dir = tmp_dir("merge_md5");
        let parts: [&[u8]; 3] = [b"AAAA", b"BBBB", b"CCCC"];
        let md5 = chunk_md5(b"different content entirely");

        for (i, p) in parts.iter().enumerate() {
            save_chunk(
                &db,
                &dir,
                &allows(),
                10 * 1024 * 1024,
                chunk_input(&md5, "a.txt", i as u32, 3, temp_file(&dir, p)),
            )
            .await
            .unwrap();
        }

        let bad = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            1,
            &merge_req(&md5, "a.txt", 3, 12),
        )
        .await;

        let chunk_dir = dir.join("chunks").join(&md5);
        let tmp_left = std::fs::read_dir(&chunk_dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains("merge_tmp"));
        let numbers = file_repo::find_chunk_numbers_by_md5(&db, &md5)
            .await
            .unwrap();

        assert!(
            matches!(bad, Err(AppError::Biz(ref m)) if m.contains("文件校验失败")),
            "实际 {bad:?}"
        );
        assert!(!tmp_left, "校验失败后 merge_tmp 应清理");
        assert_eq!(numbers, vec![0, 1, 2], "校验失败不清分片记录（保留可重试）");
    }

    #[tokio::test]
    async fn merge_chunks_persists_file_and_cleans_chunks() {
        let db = test_txn().await;
        let dir = tmp_dir("merge_ok");
        let parts: [&[u8]; 3] = [b"hello ", b"world, ", b"resume!"];
        let full: Vec<u8> = parts.concat();
        let md5 = chunk_md5(&full);

        for (i, p) in parts.iter().enumerate() {
            save_chunk(
                &db,
                &dir,
                &allows(),
                10 * 1024 * 1024,
                chunk_input(&md5, "报告.txt", i as u32, 3, temp_file(&dir, p)),
            )
            .await
            .unwrap();
        }

        let model = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            42,
            &merge_req(&md5, "报告.txt", 3, full.len() as u64),
        )
        .await
        .unwrap();

        let on_disk = std::fs::read(dir.join(&model.stored_name)).unwrap();
        let chunk_dir_gone = !dir.join("chunks").join(&md5).exists();
        let records = file_repo::find_chunk_numbers_by_md5(&db, &md5)
            .await
            .unwrap();
        let instant = file_repo::find_file_by_md5(&db, &md5).await.unwrap();

        assert_eq!(model.name, "报告.txt");
        assert_eq!(model.ext, "txt");
        assert_eq!(model.md5.as_deref(), Some(md5.as_str()), "md5 应回填入库");
        assert_eq!(model.size, full.len() as u64, "size 按实际字节数落库");
        assert_eq!(model.created_by, 42, "created_by 盖章");
        assert_eq!(on_disk, full, "合并字节应等于分片按序拼接");
        assert!(chunk_dir_gone, "分片目录应清理");
        assert!(records.is_empty(), "分片记录应清空");
        assert_eq!(instant.map(|m| m.id), Some(model.id), "合并后可秒传命中");
    }

    #[tokio::test]
    async fn merge_chunks_short_circuits_when_md5_already_exists() {
        let db = test_txn().await;
        let dir = tmp_dir("merge_instant");
        let parts: [&[u8]; 2] = [b"first-", b"second"];
        let full: Vec<u8> = parts.concat();
        let md5 = chunk_md5(&full);

        for (i, p) in parts.iter().enumerate() {
            save_chunk(
                &db,
                &dir,
                &allows(),
                10 * 1024 * 1024,
                chunk_input(&md5, "a.txt", i as u32, 2, temp_file(&dir, p)),
            )
            .await
            .unwrap();
        }
        let first = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            1,
            &merge_req(&md5, "a.txt", 2, full.len() as u64),
        )
        .await
        .unwrap();
        // 造回分片数据模拟重复提交
        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 2, temp_file(&dir, b"first-")),
        )
        .await
        .unwrap();
        let second = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            1,
            &merge_req(&md5, "a.txt", 2, full.len() as u64),
        )
        .await
        .unwrap();

        assert_eq!(first.id, second.id, "重复合并应秒传短路返回同一记录");
    }

    #[tokio::test]
    async fn merge_chunks_rejects_while_locked() {
        let db = test_txn().await;
        let dir = tmp_dir("merge_lock");
        let bytes = b"locked content";
        let md5 = chunk_md5(bytes);

        save_chunk(
            &db,
            &dir,
            &allows(),
            10 * 1024 * 1024,
            chunk_input(&md5, "a.txt", 0, 1, temp_file(&dir, bytes)),
        )
        .await
        .unwrap();

        let guard = acquire_merge_lock(&md5);
        let locked = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            1,
            &merge_req(&md5, "a.txt", 1, bytes.len() as u64),
        )
        .await;
        drop(guard);
        let released = merge_chunks(
            &db,
            &dir,
            &allows(),
            500 * 1024 * 1024,
            1,
            &merge_req(&md5, "a.txt", 1, bytes.len() as u64),
        )
        .await;

        assert!(
            matches!(locked, Err(AppError::Biz(ref m)) if m.contains("正在合并中")),
            "占坑期间应拒绝：{locked:?}"
        );
        assert!(released.is_ok(), "guard 释放后应可合并：{released:?}");
    }

    #[tokio::test]
    async fn remove_chunks_clears_dir_and_is_idempotent() {
        let db = test_txn().await;
        let dir = tmp_dir("chunk_remove");
        let md5 = chunk_md5(b"to be removed");

        for i in 0..2 {
            save_chunk(
                &db,
                &dir,
                &allows(),
                10 * 1024 * 1024,
                chunk_input(&md5, "a.txt", i, 2, temp_file(&dir, b"xx")),
            )
            .await
            .unwrap();
        }

        let removed = remove_chunks(&db, &dir, &md5).await.unwrap();
        let dir_gone = !dir.join("chunks").join(&md5).exists();
        let again = remove_chunks(&db, &dir, &md5).await.unwrap();

        assert_eq!(removed, 2, "应删除 2 条分片记录");
        assert!(dir_gone, "分片目录应删除");
        assert_eq!(again, 0, "重复 remove 幂等返回 0");
    }
}
