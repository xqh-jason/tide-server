# W5-6 系统配置模块设计规格

> 日期：2026-09-05
> 状态：待用户 review
> 模块位置：W5 通用模块第 6 项（…… 文件上传 → 图形验证码 → **系统配置**，W5 收尾）

## 1. 背景与目标

系统配置 = **键值参数配置**（运行时可增删改查的参数）+ **网站设置**（单行站点级
配置，登录页等公开场景可读）。对齐 GVA 的 sys_config / sys_system 能力，
遵守本项目既有约定：POST + JSON body、软删、审计盖章、codegen 出表、TDD。

## 2. 决策记录

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 数据形态 | **两者都做**：`sys_config` 键值参数表 + `sys_system` 单行网站设置表 |
| 2 | 网站设置字段 | GVA sys_system 对齐全量（11 业务字段，见 §3.2），后续加字段走迁移 |
| 3 | 读取权限 | 网站设置 `GET /site-config/get` **公开**（登录页展示站点名/logo）；其余端点全部登录态 |
| 4 | 域名 | `src/modules/config/`（避开已存在的 `system` 域名） |
| 5 | 单行表实现 | sys_system 手写 entity + service（codegen 模板是 CRUD 型，不适用单行） |
| 6 | 单行保障 | 迁移种子插入 `id=1` 默认行；更新恒按 id=1，不提供 create/delete |
| 7 | cache 联动 | 不做（配置量小直查库，backlog） |
| 8 | 权限码按钮 | 不做（后续 API 授权层统一补齐） |

## 3. 表结构

### 3.1 `sys_config`（键值参数，codegen）

| 列 | 类型 | 约束 / 默认 | 说明 |
|---|---|---|---|
| id | BIGINT UNSIGNED | PK, AUTO_INCREMENT | |
| config_name | VARCHAR(64) | NOT NULL DEFAULT '' | 参数名称（展示） |
| config_key | VARCHAR(64) | NOT NULL | 参数键（业务内唯一，含软删占位） |
| config_value | VARCHAR(255) | NOT NULL DEFAULT '' | 参数值 |
| remark | VARCHAR(255) | NOT NULL DEFAULT '' | 备注 |
| created_by / updated_by | BIGINT UNSIGNED | NOT NULL DEFAULT 0 | 审计盖章 |
| created_at / updated_at | DATETIME | 时间戳默认 | |
| deleted_at | DATETIME | NULL | 软删 |

索引：`uk_sys_config_config_key`（唯一）、`idx_sys_config_created_at`。

### 3.2 `sys_system`（网站设置，单行 id=1，手写迁移 + entity）

| 列 | 类型 | 说明 |
|---|---|---|
| id | BIGINT UNSIGNED PK | 恒为 1 |
| name | VARCHAR(64) | 站点名称 |
| logo | VARCHAR(255) | logo 图片 URL（可来自 W5-4 上传） |
| ico | VARCHAR(255) | 浏览器 tab 图标 URL |
| watermark_text | VARCHAR(64) | 水印文字 |
| watermark_enable | TINYINT(1) | 水印开关 0/1 |
| watermark_type | VARCHAR(16) | 水印类型（text / pic） |
| watermark_pic | VARCHAR(255) | 水印图片 URL |
| mode | VARCHAR(8) | 主题白黑：white / black |
| side_mode | VARCHAR(8) | 侧边栏模式：dark / light / head |
| color | VARCHAR(16) | 主题色 |
| created_by / updated_by / created_at / updated_at / deleted_at | | 审计与软删（单行实际不软删，列保持表结构一致） |

种子：迁移 `up` 插入 `id=1` 默认行（`INSERT IGNORE` 语义，`down` 随表删除）。

## 4. 接口契约

### 4.1 参数配置（`/api/v1/config/*`，AuthRequired + OperationLog）

| 端点 | 请求 | 说明 |
|---|---|---|
| POST /config/list | `{page, keyword?}` | keyword 模糊匹配 name / key，created_at 倒序 |
| POST /config/create | `{config_name, config_key, config_value, remark?}` | key 重复（含软删占位）→ Biz("配置键已存在：{key}") |
| POST /config/update | `{id, config_name, config_key, config_value, remark?}` | key 唯一排除自身 |
| POST /config/get | `{id}` | |
| POST /config/delete | `{id}` | 软删 |

列表/详情 Resp 含 `created_by_name / updated_by_name`（走 fill_user_names）。

### 4.2 网站设置（`/api/v1/site-config/*`）

| 端点 | 权限 | 说明 |
|---|---|---|
| GET /site-config/get | **公开** | 返回 id=1 整行（无 body、无分页）；行不存在自动按默认值兜底返回 |
| POST /site-config/update | AuthRequired | 全量字段提交，恒更新 id=1；repo 盖章 updated_by |

## 5. 模块组织

`src/modules/config/`：`mod.rs`（`routes()` 参数 CRUD + `site_routes()` 网站设置）、
`api.rs` / `service.rs` / `repo.rs` / `dto.rs` 内部分「参数（Param）」与「网站设置（Site）」
两组。router.rs：`/config` 包 AuthRequired + OperationLog；`/site-config` 的 get
不挂中间件、update 挂 AuthRequired（参考 auth 域 logout 的子路由挂法）。

命名（避免与 captcha 等冲突，按层统一）：repo `find_param_page / find_param_by_key_... /
create_param / update_param / soft_delete_param / find_site_config / update_site_config`；
service `page_params / create_param / update_param / get_param / delete_param /
get_site_config / update_site_config`；handler `list_params / create_param / update_param /
get_param / delete_param / get_site_config / update_site_config`。

## 6. 测试策略

- **repo（连 MySQL）**：分页 keyword 过滤软删倒序；key 查重含软删占位；软删后不可见；
  site 行存在（种子）且 update 生效；
- **service（连 MySQL）**：create/update key 查重 Biz 分支；get/delete 缺失 Biz；
  site get 兜底（行缺失不报错）；update 盖章 updated_by；
- 手动冒烟：create → list 见行 → 重复 key 被拒 → delete 后 list 消失；
  site-config/get 无 token 可读；update 改 name 后 get 生效。

## 7. 范围外

- 参数读取 cache 联动（backlog）；
- 配置分组 / 值类型（string/number/bool）字段；
- 权限码按钮与菜单种子（后续 API 授权层）；
- 前端配置管理页（前端仓库）。
