# W5-4 文件上传模块设计规格

> 日期：2026-09-05
> 状态：待用户 review
> 模块位置：W5 通用模块第 4 项（操作日志 → 登录日志 → 数据字典 → **文件上传** → 图形验证码 → 系统配置）

## 1. 背景与目标

提供通用文件上传能力：前端（vben Upload 组件、用户头像等）上传文件到本地磁盘，
记录元数据并支持鉴权下载与列表管理。语义对齐 gin-vue-admin 的本地文件上传，
遵守本项目既有约定：

- 统一契约：所有接口 `POST + JSON body`（上传与下载为两个例外：multipart 与 GET）；
- 主表软删、列表分页、接口命名与路由顺序遵循垂直切片四件套约定；
- 表带审计字段（created_by/updated_by，repo 统一盖章），名称拼装走
  `utils::user_ref` 管道；
- 代码骨架由生成器产出，业务实现以 TDD 推进。

## 2. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 存储后端 | **本地磁盘**，目录由 config 配置（`upload.dir`），开发与生产一致 |
| 2 | 元数据 | 建 `sys_file` 记录表，支持列表 / 详情 / 删除管理 |
| 3 | 访问控制 | 上传 AuthRequired；**下载走鉴权接口** `/file/download?id=`，不暴露公开静态目录 |
| 4 | 限制 | 单文件 ≤ 10 MB；扩展名白名单（可配置）；保存名 UUID 化（防覆盖与路径穿越） |
| 5 | 存储布局 | `upload.dir/<uuid>.<ext>` 单层平铺（W6 断点续传再引入日期分片） |
| 6 | 删除语义 | delete = 软删记录 + **物理删除磁盘文件**（软删记录保留可审计，但已不可下载） |
| 7 | 权限种子 | 本轮**不新增**菜单/权限码按钮——接口级权限由后续 API 授权层统一施加 |
| 8 | 下载实现 | `salvo::fs::NamedFile` 流式返回；响应 `Content-Disposition` 用原始文件名 |

## 3. 表结构

表名：`sys_file`。

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | 主键 |
| name | VARCHAR(255) | NOT NULL | 原始文件名（含扩展名，下载时用于 Content-Disposition） |
| stored_name | VARCHAR(255) | NOT NULL | 磁盘存储名：`<uuid>.<ext>`（唯一，列表按此定位文件） |
| ext | VARCHAR(20) | NOT NULL DEFAULT '' | 小写扩展名（白名单校验依据） |
| mime | VARCHAR(100) | NOT NULL DEFAULT '' | Content-Type |
| size | BIGINT UNSIGNED | NOT NULL DEFAULT 0 | 字节数 |
| created_by | BIGINT UNSIGNED | NOT NULL DEFAULT 0 | 上传人 ID（repo 盖章） |
| updated_by | BIGINT UNSIGNED | NOT NULL DEFAULT 0 | 更新人 ID（repo 盖章） |
| created_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP | |
| updated_at | DATETIME | NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE | |
| deleted_at | DATETIME | NULL | 软删时间 |

索引：

- `idx_sys_file_created_at`（created_at DESC）：列表默认倒序。

## 4. 配置

`config.toml` 新增段（`src/infra/config.rs` 同步读取并注入 `AppState`）：

```toml
[upload]
# 文件落盘目录（相对进程工作目录或绝对路径均可），生产与本地一致
dir = "./uploads"
# 单文件大小上限（MB）
max_size_mb = 10
# 扩展名白名单（小写，去点）
allows = ["png", "jpg", "jpeg", "gif", "webp", "svg", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "zip"]
```

## 5. 接口契约

路由前缀：`/api/v1/file`（AuthRequired + OperationLog）。上传为 multipart，下载为 GET，
其余 POST + JSON body。

| 端点 | 请求 | 说明 |
|---|---|---|
| POST /upload | multipart `file` 字段 | 落盘 + 记录；返回 FileResp（含 `url` 供前端直用） |
| POST /list | `{ page, keyword?, }` | 分页；keyword 对 name 模糊；默认 created_at 倒序；含上传人显示名 |
| POST /get | `{ id }` | 详情 |
| POST /delete | `{ id }` | 软删记录 + 物理删磁盘文件 |
| GET /download?id= | query | 鉴权流式下载（NamedFile） |

列表响应项：

```json
{
  "id": 1,
  "name": "报告.pdf",
  "stored_name": "a1b2c3d4-....pdf",
  "ext": "pdf",
  "mime": "application/pdf",
  "size": 20480,
  "created_by": 1,
  "created_by_name": "admin",
  "created_at": "2026-09-05 10:00:00",
  "updated_at": "2026-09-05 10:00:00"
}
```

上传响应在列表字段基础上含 `url`（`/api/v1/file/download?id={id}`，前端拼接站点
域名后可直接使用）。

不存在记录时 get / delete / download 返回业务错误：

- `文件不存在：{id}`。

## 6. 上传流程与校验

```text
POST /upload（multipart, field=file）
  → AuthRequired 注入操作人
  → 解析 FormData：取 file 字段 FilePart
  → 校验链：
      1. 存在性：无文件字段 → Biz("未选择文件")
      2. 扩展名白名单：从原始文件名取 ext 小写，不在 allows → Biz("不支持的文件类型：{ext}")
      3. 大小：FilePart::size > max_size → Biz("文件大小超出限制：{size} 字节")
  → 生成 uuid 文件名：format!("{uuid}.{ext}")（uuid v4，取简单 hex）
  → 拷贝临时文件到 upload.dir/<stored_name>（FilePart 落盘路径即临时文件，用 fs::rename 或 copy）
  → 写 sys_file 记录（created_by = 上传人，repo 盖章双写）
  → 返回 FileResp
```

实现要点：

- FilePart 解析后自带临时文件路径（`salvo::http::form`），处理完会自动清理；
  落盘用 `tokio::fs::rename`（同盘）或 `copy` + 清理临时文件；
- `upload.dir` 目录启动时不预建，首传前 `create_dir_all`；
- 校验放 service（业务规则），repo 只负责数据访问；
- 下载：`NamedFile::builder(path).build()`，文件头（原始名、mime、size）从记录取，
  不信任路径参数（id → 记录 → stored_name → 磁盘路径，禁止前端直传路径）。

## 7. 错误处理

- 类型/大小超限、文件不存在等业务校验 → `AppError::Biz`（code=0，message 见上）；
- 磁盘 IO 失败（rename/read）→ `AppError::Internal`；
- download 中文件记录存在但磁盘文件缺失（异常删除）→ `AppError::Biz("文件存储已丢失：{id}")`。

## 8. 测试策略

### 8.1 repo 集成测试

- 分页按 keyword 过滤并排除软删、created_at 倒序；
- soft_delete 置 deleted_at。

### 8.2 service 测试

- 上传校验：无扩展名 / 白名单外 / 超大小 / 无文件字段各自返回 Biz；
- 上传成功：uuid 命名、落盘文件存在、记录 created_by = 上传人；
- 删除：软删记录 + 磁盘文件物理删除；
- get/delete 不存在返回 Biz。

> 说明：service 上传测试直连 MySQL 但对磁盘只读校验白名单与命名；
> 落盘与物理删除的完整链路放 handler 集成冒烟（手动）或 repo 层夹具。
> 测试用唯一上传目录或测后清理 `upload.dir` 下的测试文件。

### 8.3 handler 冒烟（可选，手动）

- curl multipart 上传 → 拿 url → 带 token GET download 能取回同字节；
- 无 token 下载返回 401；
- 列表带 created_by_name。

## 9. 范围外

- MinIO / 对象存储（预留后端抽象，后续需要时引入）；
- 断点续传 / 分块合并（W6）；
- 前端 Upload 组件对接页（前端仓库）；
- 权限码按钮与菜单（后续 API 授权层模块统一补齐）。

## 10. 参考实现位置

- 迁移注册：`migrations/src/lib.rs`
- 路由挂载：`src/infra/router.rs`
- 配置读取：`src/infra/config.rs`、`src/infra/state.rs`（AppState 注入 upload 配置）
- multipart / 文件响应 API：`salvo::http::form::{FormData, FilePart}`、
  `salvo::fs::NamedFile`（无需新增 feature）
- 名称拼装管道：`src/utils/user_ref.rs`
- codegen 域定义：`codegen/defs/file.json`
