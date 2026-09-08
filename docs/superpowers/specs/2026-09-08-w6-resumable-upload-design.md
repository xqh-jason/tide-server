# W6-3 文件断点续传设计规格

> 日期：2026-09-08
> 状态：待用户 review
> 模块位置：W6 扩展模块第 3 项（接口级授权层 → 定时任务 → **断点续传**；服务器监控已决策跳过）

## 1. 背景与目标

在 W5-4 文件上传（`sys_file` + 本地磁盘存储）基础上补齐大文件场景：分片上传、
断点续传（重试时跳过已传分片）、秒传（同 md5 文件已存在则跳过上传）、合并校验。
语义对齐 gin-vue-admin 的 `fileUploadAndDownload` 断点续传四端点
（breakpointContinue / findFile / breakpointContinueFinish / removeChunk），
端点命名按本项目契约重排。遵守既有约定：

- `POST + JSON body` 为主，**分片上传是 multipart 例外**（同 W5 上传）；
- 业务校验放 service、磁盘操作放 service、repo 只管库；
- 分片上传/合并走 file 域现有中间件链（AuthRequired → OperationLog → ApiPermission）；
- 定时清理复用 W6-2 任务注册表（`src/task/`，新增任务 = 建文件 + 注册一行）。

## 2. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 文件指纹 | **md5**（对齐 GVA，前端 spark-md5 分块计算成熟；非安全用途仅作去重标识）。后端合并后流式重算校验，不信前端单方面声明 |
| 2 | 断点标识 | `file_md5` 全局唯一标识一次上传会话（同 md5 = 同文件，全局共享，不同用户传同文件天然秒传） |
| 3 | 分片记录表 | 新建 `sys_file_chunk`；**硬删不软删**（临时数据，无 deleted_at / 审计字段，豁免主表盖章约定） |
| 4 | 秒传支撑 | `sys_file` 加 `md5` 列（NULL + 索引）；存量记录 NULL 不参与秒传 |
| 5 | 存储布局 | 分片放 `upload.dir/chunks/<file_md5>/<chunk_number 五位补零>.part`；正式文件仍 W5 平铺 `<uuid>.<ext>`，不动 |
| 6 | 分片幂等 | 同 `file_md5 + chunk_number` 重复上传 = 覆盖写文件 + upsert 记录（断点重试天然幂等）；表加唯一键兜底并发 |
| 7 | 合并防重叠 | 同一 `file_md5` 合并互斥（dashmap entry 占坑，对齐 W6-2 job 防重叠范式）；重复合并请求先查 `sys_file.md5` 已存在 → 直接返回秒传结果 |
| 8 | 合并校验链 | 分片齐全且连续 → 按序 append 落临时文件（流式 md5 重算）→ rename 到正式名 → 写 `sys_file`（含 md5）→ 清分片目录与记录；任何一步失败清理半成品 |
| 9 | 总大小上限 | 断点续传走独立上限 `resumable_max_size_mb`（默认 500，大于普通上传 10 MB），merge 时按声明 size 校验，落库前按实际字节数复核 |
| 10 | 分片大小上限 | 单分片 ≤ `chunk_max_size_mb`（默认 10 MB，与普通上传上限共用一个数即可，配置独立可调） |
| 11 | 孤儿分片清理 | 手动 `chunk/remove`（对齐 GVA removeChunk）+ **内置定时任务 `chunk_cleanup`**（清理超过 `chunk_retain_hours`（默认 24h）未合并的分片目录与记录）双保险 |
| 12 | chunk 编号 | **0-based**，`chunk_number ∈ [0, chunk_total)`，契约文档写死（GVA 0-based） |
| 13 | 路由归位 | 全部挂在现有 `/api/v1/file` 域组内，不另开域 |

## 3. 表结构

### 3.1 新表 `sys_file_chunk`

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | 主键 |
| file_md5 | VARCHAR(32) | NOT NULL | 整文件 md5（32 位小写 hex，断点会话标识） |
| chunk_number | INT UNSIGNED | NOT NULL | 分片序号（0-based） |
| chunk_path | VARCHAR(255) | NOT NULL | 分片相对路径：`chunks/<file_md5>/00042.part` |
| created_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP | |

索引：

- `uk_sys_file_chunk_md5_number`（file_md5, chunk_number）物理唯一：upsert 依据 + 并发兜底
- 查询均按 file_md5 前缀命中，无需额外索引

### 3.2 `sys_file` 加列

- `md5` VARCHAR(32) NULL，索引 `idx_sys_file_md5`；秒传查询 `md5 = ? AND deleted_at IS NULL`

迁移：`m20260908_000015_create_sys_file_chunk_and_add_md5`（down 可回滚，含加列 + 建表）。

## 4. 配置

`config.toml` `[upload]` 段新增（`config.rs` / `state.rs` 同步透出）：

```toml
[upload]
# ... 既有 dir / max_size_mb / allows 不变 ...
# 断点续传：整文件大小上限（MB），独立于普通上传的 max_size_mb
resumable_max_size_mb = 500
# 断点续传：单分片大小上限（MB）
chunk_max_size_mb = 10
# 分片保留时长（小时）：超时未合并的分片由定时任务清理
chunk_retain_hours = 24
```

依赖新增：`md5 = "0.7"`（流式重算合并结果，先 dry-run 验证 rsproxy 有该版本）。

## 5. 接口契约

路由前缀 `/api/v1/file`，组内顺序：`list → upload → chunk-status → chunk-upload → chunk-merge → chunk-remove → get → download → delete`（读在前、写在后，对齐既有命名约定）。

| 端点 | 请求 | 说明 |
|---|---|---|
| POST /chunk/status | `{ fileMd5, fileName }` | 断点探测：返回已传分片序号列表；md5 对应完整文件已存在则 `done=true` 并带 `file`（秒传） |
| POST /chunk/upload | multipart：`file` + `fileMd5` / `fileName` / `chunkNumber` / `chunkTotal` | 保存单个分片（幂等覆盖），返回 `{ }` |
| POST /chunk/merge | `{ fileMd5, fileName, chunkTotal, size, mime? }` | 校验齐全 → 合并 → md5 复核 → 入库 → 清分片，返回 `FileUploadResp`（与普通上传一致，含 url） |
| POST /chunk/remove | `{ fileMd5 }` | 放弃上传：删分片目录 + 记录（幂等，不存在也返回成功） |

chunk/status 响应：

```json
{
  "uploaded": [0, 1, 3],
  "done": false,
  "file": null
}
```

- `done=true` 时 `file` 为 `FileUploadResp`（含 url），前端直接使用、跳过上传；
- `fileName` 仅用于合并后落 `sys_file.name` 与白名单校验，不参与分片定位。

## 6. 流程与校验

### 6.1 chunk/upload 校验链（顺序对齐 W5 上传）

```text
POST /chunk/upload（multipart）
  → AuthRequired 注入操作人
  → 解析 FormData：file 字段 + 四个文本字段
  → 校验链：
      1. fileMd5：32 位小写 hex → 否则 Biz("fileMd5 格式不合法")
      2. fileName 非空 + 扩展名白名单（复用 W5 校验，Biz 文案一致）
      3. chunkNumber < chunkTotal 且 chunkTotal ≥ 1 → 否则 Biz("分片序号越界：{n}")
      4. 分片大小 ≤ chunk_max_size_mb → 否则 Biz("分片大小超出限制：{size} 字节")
  → create_dir_all(upload.dir/chunks/<fileMd5>)
  → 覆盖写分片文件（tokio::fs::copy / rename，复用 W5 rename 降级逻辑）
  → upsert sys_file_chunk（唯一键冲突则仅覆盖文件、更新 created_at）
```

### 6.2 chunk/merge 流程

```text
POST /chunk/merge
  → 校验链：fileMd5 格式 / fileName 白名单 / size ≤ resumable_max_size_mb
  → 防重叠：dashmap 占坑 file_md5（W6-2 entry 范式），拿不到 → Biz("该文件正在合并中")
  → 秒传短路：sys_file 按 md5 查未删记录，命中 → 清理本次分片 → 直接返回已有记录 Resp
  → 齐全校验：sys_file_chunk 按 md5 查集合，与 [0, chunkTotal) 全集比对 → 缺 → Biz("分片缺失：{n}")
  → 合并：按序 append 写临时文件 merge_tmp，逐块流式喂 md5
  → md5 复核：计算结果 ≠ fileMd5 → 删临时文件 → Biz("文件校验失败，请重新上传")
  → rename merge_tmp → <uuid>.<ext>；实际字节数 ≠ 声明 size → 同上处理
  → 写 sys_file（md5 入库，created_by 盖章）
  → 清分片目录 + sys_file_chunk 记录（失败仅 warn 日志，不回滚主文件）
  → 释放占坑；返回 FileUploadResp
```

### 6.3 chunk_cleanup 定时任务

- `src/task/chunk_cleanup.rs`：扫 `sys_file_chunk` 中 `created_at < now - chunk_retain_hours`
  的记录 → 逐 md5 组删记录 + 删空目录（`remove_dir_all(chunks/<md5>)`，目录内全部分片过期才删）；
- 注册一行进 `src/task/mod.rs` 注册表，cron 默认每小时（种子示例任务之外，代码内默认注册）。

## 7. 错误处理

- 业务校验失败（md5 格式 / 白名单 / 越界 / 超限 / 缺分片 / 校验不一致 / 正在合并）→ `AppError::Biz`；
- 磁盘 IO 失败 → `AppError::Internal`（消息带上下文，同 W5 风格）；
- chunk/remove 幂等：md5 无任何分片也返回成功（前端放弃上传不必先探测）。

## 8. 测试策略

### 8.1 repo 集成测试（test_txn 风格；分片表为真删，用唯一 md5 造数防互扰）

- upsert：同 (md5, number) 二次写入不报唯一键冲突、记录数不翻倍；
- find_by_md5：只返回该会话分片、按 chunk_number 升序；
- delete_by_md5：记录清空；
- sys_file 按 md5 查秒传：命中未删、排除软删。

### 8.2 service 测试（真库 + 临时分片目录，测后清理）

- chunk/upload 校验链四类 Biz（格式 / 白名单 / 越界 / 超分片上限）+ 成功落盘 + 幂等覆盖；
- merge：成功链路（记录入库含 md5、正式文件字节拼接正确、分片目录与记录清空）；
- merge：缺分片 Biz、md5 不一致 Biz 且临时文件已清理、重复 merge 秒传短路；
- chunk/remove：目录与记录清空、幂等二次调用成功；
- status：uploaded 序号列表、done=false。

### 8.3 task 测试

- chunk_cleanup：过期分片记录 + 目录被清、未过期保留。

### 8.4 handler 冒烟（可选，手动）

- curl 按序传 3 分片 → status 返回 [0,1,2] → merge 得 url → download 字节 = 三段拼接；
- 中途断开（少传 1 片）→ status 返回缺失列表 → 续传后 merge 成功。

## 9. 范围外

- MinIO / 对象存储后端；
- 前端对接（spark-md5 分块、并发分片控制、重试队列）——另出《前端对接说明-W6-断点续传》文档；
- 全局并发合并数限流（v1 仅同 md5 互斥，需要时再加信号量）；
- 分片级 md5 校验（整文件 md5 复核已覆盖传输完整性，单分片哈希省去）。

## 10. 参考实现位置

- 迁移注册：`migrations/src/lib.rs`
- 实体：`src/entity/sys_file_chunk.rs`（codegen defs 出 entity，四件套手写）、`src/entity/sys_file.rs` 加 md5 字段
- 域内扩展：`src/modules/file/{repo,service,dto,api,mod}.rs`（分片函数并入 file 域，不新建子域目录）
- 防重叠范式：`src/modules/job/scheduler.rs`（dashmap entry 占坑）
- 任务注册：`src/task/mod.rs` + `src/task/chunk_cleanup.rs`（新建）
- 配置读取：`src/infra/config.rs`、`src/infra/state.rs`
- W5 复用点：白名单/ext 校验、rename 降级落盘、`FileUploadResp` 契约
