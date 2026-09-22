//! 审批请求体校验（纯函数，无第三方校验框架）。
//!
//! 结构同 employee / time_off 域：DTO（`dto.rs`）保持纯字段声明；这里为每个写请求提供一个
//! `validate_xxx(req, …) -> Result<(), String>`，命中规则即累积中文错误消息，多条用「；」
//! 拼接后返回。handler 在拿到 `body.into_inner()` 后立即调用（见 `api.rs`）。
//!
//! 值域来源唯一：
//! - `status` 的允许值来自平台字典（`dictionary::service::enabled_int_values`）；
//! - `biz_type` 的允许值来自字典 `approvalBizType`（字符串字典，取启用项的 `value`）；
//!   两者都由 api 层预取后作为参数传入，**不硬编码**。
//!
//! 需要查库的规则（`biz_type` 查重、模板 / 节点存在性、引用用户与角色是否存在、
//! 「最后一个节点不允许跳过」）留在 service 层。

use crate::modules::biz::hr::approval::dto::{
    ApproveReq, CreateFlowReq, UpdateFlowReq, UpsertFlowNodeReq,
};
use crate::utils::check;

/// 模板名称长度上限（对齐 `VARCHAR(64)`）。
const NAME_MAX: usize = 64;
/// 业务类型长度上限（对齐 `VARCHAR(32)`）。
const BIZ_TYPE_MAX: usize = 32;
/// 备注 / 审批意见长度上限（对齐 `VARCHAR(255)`）。
const REMARK_MAX: usize = 255;
/// 顺序号上限（与 service 的节点数上限呼应，避免配出超长链）。
const SEQ_MAX: i32 = 100;

/// 必填文本：trim 后非空 + 长度上限（按 `char` 计，避免 UTF-8 边界误判）。
fn check_required(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.trim().is_empty() {
        errors.push(format!("{label}不能为空"));
    } else if s.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 可选文本长度上限（空串放行）。
fn check_optional_len(s: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if s.chars().count() > max {
        errors.push(format!("{label}长度不能超过 {max} 个字符"));
    }
}

/// 值域检查：`{label}取值不合法，仅允许：a / b`。
fn check_int_in(value: i8, allowed: &[i8], label: &str, errors: &mut Vec<String>) {
    if allowed.contains(&value) {
        return;
    }
    let choices = allowed
        .iter()
        .map(i8::to_string)
        .collect::<Vec<_>>()
        .join(" / ");
    errors.push(format!("{label}取值不合法，仅允许：{choices}"));
}

/// 累积的错误用中文分号拼成一条消息。
fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 创建 / 更新模板共用的字段视图（两个 DTO 除 `id` 外字段同名同型）。
struct FlowFields<'a> {
    biz_type: &'a str,
    name: &'a str,
    status: i8,
    remark: &'a str,
}

/// 创建 / 更新模板共用字段检查。
fn check_flow_fields(
    fields: FlowFields<'_>,
    status_allowed: &[i8],
    biz_type_allowed: &[String],
) -> Vec<String> {
    let mut errors = Vec::new();

    let biz_type = fields.biz_type.trim();
    check_required(biz_type, "业务类型", BIZ_TYPE_MAX, &mut errors);
    if !biz_type.is_empty() && !biz_type_allowed.iter().any(|v| v == biz_type) {
        let choices = biz_type_allowed.join(" / ");
        errors.push(format!(
            "业务类型取值不合法，仅允许：{choices}（字典 approvalBizType）"
        ));
    }

    check_required(fields.name, "模板名称", NAME_MAX, &mut errors);
    check_optional_len(fields.remark, "备注", REMARK_MAX, &mut errors);
    // 状态值域的唯一来源是平台字典，文案用 check_status 的返回，不自己造句
    check::check_status(fields.status, status_allowed)
        .map_err(|e| errors.push(e))
        .ok();

    errors
}

/// 创建审批流模板校验。
///
/// 规则（命中即累积，最后 `errors.join("；")`；文案是契约，测试断言子串）：
/// - `biz_type`：trim 后非空 → `业务类型不能为空`；长度 > 32 →
///   `业务类型长度不能超过 32 个字符`；不在字典 `approvalBizType` 启用项里 →
///   `业务类型取值不合法，仅允许：…（字典 approvalBizType）`；
/// - `name`：trim 后非空 → `模板名称不能为空`；长度 > 64 → `模板名称长度不能超过 64 个字符`；
/// - `remark` 长度 > 255 → `备注长度不能超过 255 个字符`；
/// - `status` 值域由字典决定 → 文案同 `check_status`。
pub fn validate_create_flow(
    req: &CreateFlowReq,
    status_allowed: &[i8],
    biz_type_allowed: &[String],
) -> Result<(), String> {
    join_errors(check_flow_fields(
        FlowFields {
            biz_type: &req.biz_type,
            name: &req.name,
            status: req.status,
            remark: &req.remark,
        },
        status_allowed,
        biz_type_allowed,
    ))
}

/// 更新审批流模板校验：字段规则同创建 + `id == 0 → 审批流 ID 必须大于 0`。
pub fn validate_update_flow(
    req: &UpdateFlowReq,
    status_allowed: &[i8],
    biz_type_allowed: &[String],
) -> Result<(), String> {
    let mut errors = check_flow_fields(
        FlowFields {
            biz_type: &req.biz_type,
            name: &req.name,
            status: req.status,
            remark: &req.remark,
        },
        status_allowed,
        biz_type_allowed,
    );
    if req.id == 0 {
        errors.push("审批流 ID 必须大于 0".to_string());
    }
    join_errors(errors)
}

/// 新增 / 修改模板节点校验。
///
/// 规则：
/// - `flow_id == 0` → `审批流 ID 必须大于 0`；
/// - `seq <= 0` → `顺序号必须大于 0`；`seq > 100` → `顺序号不能超过 100`；
/// - `node_name` trim 后非空 → `节点名称不能为空`；长度 > 64 → `节点名称长度不能超过 64 个字符`；
/// - `node_type` ∉ {1,2,3,4} → `节点类型取值不合法，仅允许：1 / 2 / 3 / 4`；
/// - `node_type ∈ {3,4}` 时 `approver_ref_id == 0` → `指定用户 / 指定角色必须填写审批人引用 ID`
///   （用户 / 角色是否存在由 service 查库校验）；
/// - `skip_if_empty` ∉ {0,1} → `是否跳过取值不合法，仅允许：0 / 1`；
/// - `remark` 长度 > 255 → `备注长度不能超过 255 个字符`。
pub fn validate_upsert_flow_node(req: &UpsertFlowNodeReq) -> Result<(), String> {
    let mut errors = Vec::new();

    if req.flow_id == 0 {
        errors.push("审批流 ID 必须大于 0".to_string());
    }
    if req.seq <= 0 {
        errors.push("顺序号必须大于 0".to_string());
    } else if req.seq > SEQ_MAX {
        errors.push(format!("顺序号不能超过 {SEQ_MAX}"));
    }
    check_required(&req.node_name, "节点名称", NAME_MAX, &mut errors);
    check_int_in(req.node_type, &[1, 2, 3, 4], "节点类型", &mut errors);
    if matches!(req.node_type, 3 | 4) && req.approver_ref_id == 0 {
        errors.push("指定用户 / 指定角色必须填写审批人引用 ID".to_string());
    }
    check_int_in(req.skip_if_empty, &[0, 1], "是否跳过", &mut errors);
    check_optional_len(&req.remark, "备注", REMARK_MAX, &mut errors);

    join_errors(errors)
}

/// 审批动作校验（通过 / 驳回）：实例 ID > 0 + 意见长度 ≤ 255。
pub fn validate_approve(req: &ApproveReq) -> Result<(), String> {
    let mut errors = Vec::new();
    if req.id == 0 {
        errors.push("审批实例 ID 必须大于 0".to_string());
    }
    check_optional_len(&req.opinion, "审批意见", REMARK_MAX, &mut errors);
    join_errors(errors)
}
