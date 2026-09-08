# W6-3 文件断点续传实现计划

> **面向 AI 代理的工作者：** 本计划遵循协作约定——AI 交付依赖、迁移、codegen 产出、
> 骨架（签名 + `todo!()` + 步骤注释）与失败测试；用户手写业务实现；AI review 收尾。
> 步骤中用 **[AI]** / **[用户]** 标注执行者，复选框跟踪进度。
> 规格依据：`docs/superpowers/specs/2026-09-08-w6-resumable-upload-design.md`

**目标：** 在 file 域补齐分片上传 / 断点探测 / 秒传 / 合并校验 / 分片清理，md5 双向复核。
**架构：** 分片记录走新表 `sys_file_chunk`（硬删、唯一键兜底），分片文件落
`upload.dir/chunks/<md5>/`，合并复用 W5 落盘与 `FileUploadResp` 契约，清理走 W6-2 任务注册表。
**技术栈：** SeaORM 迁移 + `md5` crate 流式重算 + dashmap entry 防重叠（W6-2 范式）+ `tokio::fs`。

---

## 文件结构

创建：
- `migrations/src/m20260908_000015_create_sys_file_chunk_and_add_md5.rs`
- `codegen/defs/file_chunk.json`（仅取 entity）
- `src/entity/sys_file_chunk.rs`（codegen 生成）
- `src/task/chunk_cleanup.rs`

修改：
- `Cargo.toml`（`md5 = "0.7"`，先 dry-run 验证 rsproxy）
- `migrations/src/lib.rs`、`src/entity/mod.rs`、`src/entity/prelude.rs`
- `src/entity/sys_file.rs`（加 `md5: Option<String>`）
- `src/modules/file/{repo,service,dto,api,mod}.rs`（分片能力并入 file 域）
- `src/infra/config.rs`、`src/infra/state.rs`（`[upload]` 新增三配置）
- `src/task/mod.rs`（注册 `chunk_cleanup`）
- `config.toml`（`[upload]` 增三项）
- `src/infra/seed.rs`（幂等插入 chunk_cleanup 启用任务，见任务 7 说明）

---

## 任务 1：依赖与配置（[AI]）

- [ ] `Cargo.toml` 引入 `md5 = "0.7"`（`cargo add md5 --dry-run` 先验证镜像有货）
- [ ] `config.toml` `[upload]` 增 `resumable_max_size_mb = 500` / `chunk_max_size_mb = 10` /
  `chunk_retain_hours = 24`；`config.rs` upload struct 增三字段（serde default 兜底旧配置文件）+
  `max_size_bytes()` 同款便捷方法 `chunk_max_size_bytes()` / `resumable_max_size_bytes()`；
  `state.rs` 若 upload 配置整体注入则零改动，逐字段注入才需要补

## 任务 2：迁移（[AI]）

- [ ] `m20260908_000015_create_sys_file_chunk_and_add_md5.rs`：
  - 建 `sys_file_chunk`（id / file_md5 VARCHAR(32) / chunk_number INT UNSIGNED /
    chunk_path VARCHAR(255) / created_at；`uk_sys_file_chunk_md5_number` 唯一键；
    列中文注释对齐既有风格）
  - `sys_file` 加列 `md5 VARCHAR(32) NULL` + 索引 `idx_sys_file_md5`
  - `down` 可回滚（drop 表 + drop 列/索引）；注册 lib.rs，跑 up 后 information_schema 验证

## 任务 3：entity（[AI]）

- [ ] `codegen/defs/file_chunk.json`（无 audit 无软删声明）→ 生成器仅取 entity
  → `src/entity/sys_file_chunk.rs`，注册 mod.rs / prelude.rs（踩坑提醒：生成后检查
  `r#type` 裸标识符与 `src/` 路径前缀，本表无 type 字段应不受影响）
- [ ] `src/entity/sys_file.rs` 手工加 `pub md5: Option<String>`（Column 不变，Null 落库即可）

## 任务 4：repo 层（[AI] 失败测试 + 骨架 → [用户] 实现）

`file/repo.rs` 追加（既有 11 个函数不动）：

```rust
// 分片 upsert：唯一键 (file_md5, chunk_number) 冲突则覆盖 chunk_path 并刷新 created_at
pub async fn upsert_chunk(
    db: &impl ConnectionTrait, file_md5: &str, chunk_number: u32, chunk_path: &str,
) -> anyhow::Result<()>;
// 该会话已传分片序号，按 chunk_number 升序
pub async fn find_chunk_numbers_by_md5(
    db: &impl ConnectionTrait, file_md5: &str,
) -> anyhow::Result<Vec<u32>>;
// 硬删该会话全部分片，返回删除行数
pub async fn delete_chunks_by_md5(db: &impl ConnectionTrait, file_md5: &str) -> anyhow::Result<u64>;
// 秒传：md5 命中未删文件记录（放 repo 既有查询区）
pub async fn find_file_by_md5(db: &impl ConnectionTrait, file_md5: &str)
    -> anyhow::Result<Option<sys_file::Model>>;
// 清理用：早于 cutoff 的分片所属 md5 去重列表
pub async fn find_expired_chunk_md5s(
    db: &impl ConnectionTrait, cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<Vec<String>>;
```

- [ ] **[AI]** 失败测试（`test_txn` 事务回滚风格；分片表真删，造数用唯一 md5 前缀防互扰）：
  - `upsert_chunk_overwrites_path_without_duplication`（二次 upsert 行数不翻倍、path 已覆盖）
  - `find_chunk_numbers_by_md5_returns_sorted_and_scoped`（只含该 md5、升序）
  - `delete_chunks_by_md5_clears_rows`
  - `find_file_by_md5_hits_undeleted_only`（软删不命中）
  - `find_expired_chunk_md5s_returns_old_only`（cutoff 边界：旧 md5 在列、新 md5 不在）
- [ ] **[用户]** 实现五函数 → **[AI]** 复跑 `cargo test file` 转绿

## 任务 5：service 层（[AI] 失败测试 + 骨架 → [用户] 实现）

`file/service.rs` 追加。模块级防重叠占坑（W6-2 dashmap entry 范式，Drop 自动释放）：

```rust
static MERGE_LOCKS: OnceLock<DashMap<String, ()>> = OnceLock::new();

struct MergeGuard(String);          // 占坑即插入，Drop 时 remove
impl Drop for MergeGuard { /* remove_by_key */ }

pub async fn chunk_status(
    db: &impl ConnectionTrait, req: &ChunkStatusReq,
) -> Result<(Vec<u32>, Option<sys_file::Model>), AppError>;
// (已传序号, 秒传命中记录)；done 由 handler 按 Option 判断

pub struct ChunkUploadInput {
    pub file_md5: String, pub file_name: String,
    pub chunk_number: u32, pub chunk_total: u32,
    pub size: u64, pub temp_path: PathBuf,
}
pub async fn save_chunk(
    db: &impl ConnectionTrait, dir: &Path, allows: &[String],
    chunk_max_bytes: u64, input: ChunkUploadInput,
) -> Result<(), AppError>;
// 校验链：md5 32 位小写 hex → 白名单(复用 W5 ext 提取与文案) →
// chunk_total ∈ [1, 10_000] 且 chunk_number < chunk_total（补充防御，spec §6.1 之外）
// → 分片大小 ≤ chunk_max_bytes；落盘 chunks/<md5>/<number:05>.part + upsert

pub async fn merge_chunks(
    db: &impl ConnectionTrait, dir: &Path, allows: &[String],
    resumable_max_bytes: u64, actor_id: u64, req: &ChunkMergeReq,
) -> Result<sys_file::Model, AppError>;
// spec §6.2 全链路：校验 → 占坑(拿不到→Biz"该文件正在合并中") → 秒传短路 →
// 齐全校验(缺→Biz"分片缺失：{n}") → 按序 append 临时文件 + 流式 md5 →
// md5/字节数复核(失败清理临时文件→Biz) → rename <uuid>.<ext> →
// 写 sys_file(md5 入库、mime 缺省 application/octet-stream、created_by 盖章) →
// 清分片目录 + delete_chunks_by_md5(失败仅 warn 不回滚)

pub async fn remove_chunks(
    db: &impl ConnectionTrait, dir: &Path, file_md5: &str,
) -> Result<u64, AppError>;   // 幂等：删目录(remove_dir_all 缺失不报错) + 删记录，返回行数
```

- [ ] **[AI]** 失败测试（真库 + 独立临时分片目录，测后清理；service 测试前缀 `svc_` 防并发撞键）：
  - `save_chunk_rejects_bad_md5_format`
  - `save_chunk_rejects_ext_outside_allowlist`（文案含"不支持的文件类型"）
  - `save_chunk_rejects_chunk_number_out_of_range`（number ≥ total、total = 0、total > 10_000）
  - `save_chunk_rejects_oversize_chunk`
  - `save_chunk_overwrites_same_number_idempotently`（二次覆盖文件内容 + 记录不翻倍）
  - `chunk_status_lists_uploaded_and_hits_instant_upload`（2/3 片 uploaded=[0,1]；先 merge 后
    status 得秒传 Model）
  - `merge_chunks_rejects_missing_chunks`
  - `merge_chunks_rejects_md5_mismatch_and_cleans_tmp`（分片内容与声明 md5 不符；断言
    merge_tmp 已清理、分片记录仍在可重试）
  - `merge_chunks_persists_file_and_cleans_chunks`（三片字节拼接正确、md5 入库、
    created_by=actor、分片目录与记录清空）
  - `merge_chunks_short_circuits_when_md5_already_exists`（先成功一次再 merge → 返回同 id）
  - `merge_chunks_rejects_while_locked`（手动占坑后调用得 Biz"正在合并中"）
  - `remove_chunks_clears_dir_and_is_idempotent`
- [ ] **[用户]** 实现 → **[AI]** 复跑 `cargo test file` 全绿（含既有 W5 测试无回归）

## 任务 6：dto / api / 路由（[用户]，AI 补 OpenAPI 注释 review）

- [ ] `dto.rs` 追加（`rename_all = "camelCase"`）：
  `ChunkStatusReq { file_md5, file_name }`、
  `ChunkStatusResp { uploaded: Vec<u32>, done: bool, file: Option<FileUploadResp> }`、
  `ChunkUploadInput`（见任务 5）、
  `ChunkMergeReq { file_md5, file_name, chunk_total, size, mime: Option<String> }`、
  `ChunkRemoveReq { file_md5 }`
- [ ] `api.rs` 追加 4 个 handler（端点顺序 = 路由顺序）：
  `chunk_status → chunk_upload → chunk_merge → chunk_remove`；
  `chunk_upload` 为 multipart（同 `upload_file` 范式：`form.files.get("file")` +
  `form.fields` 取 `fileMd5/fileName/chunkNumber/chunkTotal` 文本字段，parse 失败 →
  Biz("分片参数缺失或不合法")）；`chunk_merge` 成功后经 `fill_user_names` 组装
  `FileUploadResp`（含 url），与普通上传响应完全同构
- [ ] `mod.rs` 路由组在 `upload` 之后按序插入
  `/chunk/{status,upload,merge,remove}`（既有中间件链自动覆盖，router.rs 零改动）

## 任务 7：chunk_cleanup 定时任务（[AI] 骨架 → [用户] run 实现）

- [ ] `src/task/chunk_cleanup.rs`：`run(state: Arc<AppState>)` —— 每轮
  `find_expired_chunk_md5s(now - chunk_retain_hours)` → 逐 md5：
  `remove_dir_all(chunks/<md5>)`（缺失不报错）+ `delete_chunks_by_md5`；失败仅 warn 继续
- [ ] `src/task/mod.rs` HANDLERS 注册 `"chunk_cleanup"`；
  **DB 仍是事实来源**：`ensure_seed` 幂等补插启用任务（handler_name=`chunk_cleanup`、
  cron `0 0 * * * *` 每小时、name 中文注释）——seed 测试断言按 id 归属、勿用精确计数
  （W6-2 教训）

## 任务 8：收尾（[AI]）

- [ ] 全量验证：`cargo fmt --check` / `cargo check --all-targets`（0 警告）/
  `cargo test`（双 crate 全绿）/ `cargo doc`（0 警告）
- [ ] review 重点：校验链顺序与 spec §6.1 一致、merge 失败路径临时文件必清理、
  MergeGuard 全路径释放（含 `?` 早退）、md5 入库小写、resp 契约与 W5 同构、
  幂等语义（upsert / remove / 秒传短路）
- [ ] 冒烟建议（手动）：curl 3 分片 → status=[0,1,2] → merge 得 url → download 字节=拼接；
  少传 1 片断点重试；同文件二次 status 秒传
- [ ] 运维步骤提醒：新 4 端点经 sys-api 登记接管（fail-open → 显式授权），登记清单：
  `POST /api/v1/file/chunk/{status,upload,merge,remove}`
- [ ] Commit：`feat(file): 断点续传（分片上传/秒传/合并校验/定时清理，W6-3）` + 打卡

---

## 实现提示（避免踩坑）

- multipart 文本字段：`FormData.fields` 是 `MultiMap<String, String>`，
  `form.fields.get("fileMd5")` 取 `Option<&String>`；文件字段在 `form.files`，二者并存
- md5 流式重算：`md5::Context::new()` 逐块 `consume(&buf)`，收尾
  `finalize()` → `format!("{:x}")` 得 32 位小写 hex；读文件用
  `tokio::fs::File` + 8 KB buffer 循环（勿一次 read_to_end，500 MB 文件会吃内存）
- 按序 append：`tokio::fs::OpenOptions::new().append(true).create(true)` 逐片
  `tokio::fs::copy(chunk_path, &mut target)` 或读块写块；分片缺失时**
  先齐全校验再开写**，避免半途发现缺失还要清临时文件
- `number:05` 补零：`format!("{:05}.part", chunk_number)`，排序与调试友好
- rename 降级（CrossesDevices → copy + 删临时文件）复用 W5 `upload_file` 既有逻辑，
  可提私有 helper `persist_temp(temp, dest)` 供两处共用
- 分片 upsert：`InsertResult` 捕获唯一键冲突（SeaORM `DbErr::Exec` 判
  duplicate entry 1062）或先查后插 + 唯一键兜底（测试并发下 TOCTOU 可接受——
  覆盖语义等价），二选一，倾向后者简单
- 秒传查询与软删：`find_file_by_md5` 必须 `deleted_at IS NULL`；md5 列 NULL
  （存量记录）`WHERE md5 = ?` 天然不命中，无需特判
- chunk_total 上限 10_000 是计划补充防御（spec 未列），500 MB / 10 MB 分片 = 50 片，
  余量充足

## 验收标准

- 新增测试全红 → 全绿；双 crate 全量测试通过；check / doc 0 警告
- 冒烟：分片续传、秒传、md5 不一致拒收、清理任务生效
- 前端对接说明文档（`docs/前端对接说明-W6-断点续传.md`）另出，不在本计划
