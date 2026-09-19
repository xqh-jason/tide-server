## OVERVIEW

`codegen/` — 独立 crate（非 workspace 成员）：一份域定义 JSON → 生成 entity + 域四件套骨架，是本仓脚手架的代码生成器。

**建档理由**：得分 9（独立 crate、自带 `Cargo.toml` / `Cargo.lock`，构建与测试路径与主 crate 完全独立）。

## WHERE TO LOOK

| 任务 | 位置 | 备注 |
|---|---|---|
| CLI 入口与用法 | `src/main.rs` | `codegen generate <def.json> [root]`，`root` 缺省 `..`（即项目根） |
| 定义结构与校验 | `src/def.rs` | `DomainDef` / `FieldDef` / `FilterDef` + `validate()` |
| 类型与标识符映射 | `src/typing.rs` | `entity_type` / `column_ident`（`deleted_at` → `DeletedAt`）/ `finder_param_type` |
| 模板（最大文件 1273 行） | `src/templates.rs` | 6 个 `render_*`：entity / repo / service / dto / api / mod；repo 与 service 模板内嵌整份生成的测试文件 |
| 写盘编排 | `src/generate.rs::write_all` | 生成 6 个文件到 `root/src/entity/` 与 `root/src/modules/<group>/<domain>/` |
| 已有域定义（9 份） | `defs/*.json` | config、dictionary、dictionary_detail、file、job、job_log、login_log、operation_log、position |

## CONVENTIONS

- 运行：`cargo run --manifest-path codegen/Cargo.toml -- generate defs/<域>.json ..`；测试：`cargo test --manifest-path codegen/Cargo.toml`（16 个 `#[test]`：def 4 / templates 9 / typing 2 / generate 1）。
- 依赖只有 `serde` + `serde_json` + `anyhow`，edition 2024；不依赖 sea-orm / salvo，纯文本生成。
- `def.json` 字段语义：`group` 缺省 `system`（生成到平台容器）；`audit: true` 的字段不进创建/更新请求体但进响应体；`readonly: true` 不进请求体；`unique_fields` 生成查重原语；`filters[].kind` 只能是 `exact` 或 `keyword`（`validate()` 强制）。
- 生成成功后 CLI 打印三步装配提示（`src/entity/mod.rs` 加 `pub mod <表>;`、`src/modules/<group>/mod.rs` 加 `pub mod <域>;`、`DOMAINS` 追加一行），生成器**不改**主 crate 任何文件。
- 生成的 repo 恒含 `find_by_id`（带软删过滤）、`find_page`、`create_/update_/soft_delete_<域>`，与 `src/modules/AGENTS.md` 的按层命名表一致。
- 生成物是**骨架**，交付前人工裁剪：现有域文件头留有 `codegen 生成后裁剪` / `codegen 生成后对齐规范` 字样。

## ANTI-PATTERNS

- 验证生成结果时必须传临时目录作 `root`，否则会误覆盖手写模块。
- 不在 `defs/*.json` 里写 `FieldDef` 未声明的键：serde 静默忽略（现存例子 `defs/job.json` 的 `"soft_delete": true`），既无效也不报错。
- 别指望重新生成手写域：`user` / `menu` / `dept` / `role` / `sys_api` / `refresh_token` / `auth` / `permission` / `captcha` / `health` 都没有 def 文件。
- CI 与 Dockerfile 都不构建、不测试本 crate——改动只能靠本地 `cargo test --manifest-path codegen/Cargo.toml` 兜住。
- 本 crate 不在任何 workspace 里（根 crate、`codegen`、`migrations` 是三个独立包）：只能用 `--manifest-path codegen/Cargo.toml` 驱动，`-p` / `--workspace` 类目标在这里无意义。
