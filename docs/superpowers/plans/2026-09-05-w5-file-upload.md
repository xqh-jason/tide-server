# W5-4 文件上传模块实现计划

> **面向 AI 代理的工作者：** 本计划遵循用户指示——**Rust 实现全部由用户手写**，
> AI 只提供任务拆分、文件清单、签名骨架与失败测试；用户实现后由 AI 做 review。
> 步骤中用 **[AI]** / **[用户]** 标注执行者，复选框跟踪进度。
> 规格依据：`docs/superpowers/specs/2026-09-05-w5-file-upload-design.md`

---

## 文件结构

创建：
- `migrations/src/m20260905_000011_create_sys_file.rs`（建表 + 索引 + 注释）
- `codegen/defs/file.json`（域定义，含 audit 字段）
- `src/entity/sys_file.rs`（codegen 生成）
- `src/modules/file/{mod,api,service,repo,dto}.rs`（手写）
- `src/infra/config.rs` 增 `[upload]` 段读取

修改：
- `migrations/src/lib.rs`、`src/entity/mod.rs`、`src/entity/prelude.rs`
- `src/modules/mod.rs`、`src/infra/router.rs`（挂 `file` 路由组）
- `src/infra/state.rs`（AppState 注入 upload 配置）
- `config.toml`（增 `[upload]`）

---

## 任务 1：配置 `[upload]`（[用户]）

`config.toml` 增 `[upload] dir = "./uploads" / max_size_mb = 10 / allows = [...]`；
`config.rs` 增对应 struct + serde 默认；`state.rs` 的 `AppState` 加字段并在启动注入。

## 任务 2：迁移建表（[用户]）

`m20260905_000011_create_sys_file.rs`，按规格 §3（id/name/stored_name/ext/mime/
size + created_by/updated_by + created_at/updated_at/deleted_at，
`idx_sys_file_created_at` 倒序），注册 lib.rs。跑 up 后 information_schema 验证。

## 任务 3：entity + codegen defs（[用户]）

`codegen/defs/file.json`：字段与规格一致，created_by/updated_by 标
`readonly+audit`，filters 加 `keyword→name`（若生成器不支持自定义则手写分页）。
运行生成器产出 entity（若覆盖 modules 记得只留 entity）。

## 任务 4：repo 层（[AI] 失败测试 → [用户] 实现）

`file/repo.rs`：
- `find_page(db, filter, page_index, page_size)`：keyword 对 name 模糊、排除软删、
  默认 created_at 倒序；
- `find_by_id(db, id)`（排除软删）；
- `create_file(db, model, actor_id)`（repo 盖章双写）；
- `soft_delete_file(db, id)`（置 deleted_at）。

**[AI] 失败测试**：分页过滤排除软删、倒序；soft_delete 后不可见。

## 任务 5：service 层（[AI] 失败测试 → [用户] 实现）

`file/service.rs`：
- `page_files(db, req)`：组装 filter → repo 分页；
- `upload_file(db, actor_id, name, ext, mime, size, temp_path)`：
  uuid 命名 → 落盘（`tokio::fs` rename/copy）→ 写记录 → 返回 Model；
  磁盘操作放 service（业务规则层，repo 只管库）；
- `get_file` / `delete_file`（不存在 → Biz）；
- `download_path(db, id)`：查记录 → 拼磁盘绝对路径（防路径注入靠 stored_name 由
  服务端生成，不信任入参）。

**[AI] 失败测试**：
- 扩展名白名单外返回 Biz；无文件/空名 Biz；大小超限 Biz；
- 上传成功 uuid 命名 + 记录 created_by = actor；
- delete 后记录不可见；不存在 Biz。
（落盘测试用独立临时目录，测后清理。）

## 任务 6：api / mod（[用户]）

handler 顺序 = mod.rs 路由顺序：
`list_files → upload_file → get_file → download_file → delete_file`；
路由 `/api/v1/file/{list,upload,get,download,delete}`，AuthRequired+OperationLog。
上传/下载为特殊端点：upload 取 multipart，download 用 GET query + `NamedFile`。
列表 Resp 接 `fill_user_names`（uploader 名称）；Resp 带 created_at/updated_at。

## 任务 7：router / mod 注册 + 全量验证

挂载路由、编译、跑全量测试；手动冒烟（可选）：curl 上传 → 带 token 下载一致。
**[AI]** 最终 review：校验链、uuid 命名、落盘/删除、下载响应头、审计盖章、
分页倒序、命名一致（`sys_file`/`file`/`/file`）。

---

## 实现提示（Salvo API，避免踩坑）

- multipart 解析：`let mut form = req.form::<salvo::http::form::FormData>().await?`，
  遍历 field 找 `name=="file"` 的 `FilePart`；`part.path()` 即临时文件路径
  （作用域结束自动清理，落盘需 rename/copy 出去）；
- 响应文件：`salvo::fs::NamedFile::builder(path).build()`，头信息自己写
  `Content-Type` / `Content-Disposition: attachment; filename*=UTF-8''{...}`；
- `create_dir_all` 在上传路径兜底；uuid 可用 `uuid` crate（如需添加依赖需说明，
  或退而求其次用时间戳 + 随机 hex，避免新依赖）。
