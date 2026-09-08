//! 文件业务：上传校验链、uuid 命名落盘、软删 + 物理删除、下载路径解析。
//!
//! 磁盘操作放本层（业务规则），repo 只管库。规格依据：
//! docs/superpowers/specs/2026-09-05-w5-file-upload-design.md §6 / §7 / §8.2。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use sea_orm::ActiveValue::Set;
use sea_orm::ConnectionTrait;

use crate::entity::sys_file;
use crate::modules::file::dto::{FileFilter, FileListReq, UploadInput};
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
    // 其余错误直接返回内部错误。
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
/// 临时文件 salvo 请求结束自动清理，copy 分支主动删只是让磁盘早一点释放。
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
}
