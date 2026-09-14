//! 部门业务编排层：树规则（父存在性 / 防环 / 同父同名 / 删除占用）与树组装。
//!
//! `*_in_tx` 不管理事务边界：由 api 入口 begin/commit，测试用外层事务包裹。
//! 树组装在内存完成（一次全量查询 + HashMap 递归），不做 N+1 查询。

use std::collections::HashMap;

use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::sys_dept;
use crate::modules::system::dept::dto::{CreateDeptReq, DeptLeader, DeptResp, UpdateDeptReq};
use crate::modules::system::dept::repo as dept_repo;
use crate::utils::error::AppError;
use crate::utils::user_ref::{UserRefNames, find_user_name_map_by_ids};

/// 根部门的父 path 占位（`parent_id = 0` 场景）。
const ROOT_PARENT_PATH: &str = "/0/";

/// 树组装最大深度：超出即截断（防异常脏数据成环导致无限递归 / 栈溢出，仿 menu 域）。
const MAX_DEPT_TREE_DEPTH: usize = 64;

/// 事务内创建部门：父存在性 + 同父同名校验后委托 repo 落库（含 path 回写）。
///
/// 规则：
/// - `parent_id = 0` 为根部门，path 占位 `/0/`；
/// - 父部门不存在或已软删 → `AppError::Biz`；
/// - 同父下部门名重复（含软删占位）→ `AppError::Biz`；
/// - `parent_path` 由本层组装（新父 `dept_path`，根传占位 `/0/`），天然以 `/` 结尾。
pub async fn create_dept_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    // 父部门校验：parent_id = 0 为根部门（无父），否则须存在且未软删
    let parent_path = if req.parent_id == 0 {
        ROOT_PARENT_PATH.to_string()
    } else {
        let Some(parent) = dept_repo::find_by_id(txn, req.parent_id).await? else {
            return Err(AppError::Biz("上级部门不存在或已删除".to_string()));
        };
        parent.dept_path
    };

    // 同父同名查重（同父唯一；软删占位由唯一键兜底，这里给友好错误）
    if dept_repo::find_by_parent_and_name(txn, req.parent_id, &req.dept_name)
        .await?
        .is_some()
    {
        return Err(AppError::Biz("同层级下部门名称已存在".to_string()));
    }

    let model = dept_repo::create_dept_in_tx(
        txn,
        sys_dept::ActiveModel {
            parent_id: Set(req.parent_id),
            dept_name: Set(req.dept_name.clone()),
            sort: Set(req.sort),
            status: Set(req.status),
            allow_peer_read: Set(req.allow_peer_read),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        &parent_path,
        actor_id,
    )
    .await?;
    Ok(model)
}

/// 事务内更新部门：字段更新；`parent_id` 变更即移动子树（防环 + 重算 path）。
///
/// 规则：
/// - 目标部门不存在 → `AppError::Biz`；
/// - 新父不存在或已软删 → `AppError::Biz`；
/// - 新父为自身或其下级（`new_parent.dept_path` 以自身 `dept_path` 为前缀）→ `AppError::Biz`；
/// - 同父同名（排除自身）→ `AppError::Biz`；
/// - 传给 repo 的 `new_parent_path` 取新父 `dept_path`（根部门传占位 `/0/`），
///   天然以 `/` 结尾——入参约定由本层保证，repo 不做判断。
pub async fn update_dept_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    let Some(dept) = dept_repo::find_by_id(txn, req.id).await? else {
        return Err(AppError::Biz("部门不存在".to_string()));
    };

    // 新父校验：parent_id = 0 为根；否则须存在未软删，且不得为自身或自身下级（防环）
    let new_parent_path = if req.parent_id == 0 {
        ROOT_PARENT_PATH.to_string()
    } else {
        let Some(parent) = dept_repo::find_by_id(txn, req.parent_id).await? else {
            return Err(AppError::Biz("上级部门不存在或已删除".to_string()));
        };
        // 防环：自身 path 是新父 path 的前缀 ⇒ 新父为自身或自身下级
        if parent.dept_path.starts_with(&dept.dept_path) {
            return Err(AppError::Biz("上级部门不能是自身或其下级部门".to_string()));
        }
        parent.dept_path
    };

    // 同父同名查重（排除自身）
    if let Some(existing) =
        dept_repo::find_by_parent_and_name(txn, req.parent_id, &req.dept_name).await?
        && existing.id != req.id
    {
        return Err(AppError::Biz("同层级下部门名称已存在".to_string()));
    }

    // 字段更新（窄写：只 Set 业务字段；parent_id / dept_path 由移动原语处理）
    let mut updated = dept_repo::update_dept_in_tx(
        txn,
        sys_dept::ActiveModel {
            id: Set(req.id),
            dept_name: Set(req.dept_name.clone()),
            sort: Set(req.sort),
            status: Set(req.status),
            allow_peer_read: Set(req.allow_peer_read),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;

    // 父变更 = 移动子树：委托 repo 按新父链重算整棵子树 path
    if req.parent_id != dept.parent_id {
        let moved =
            dept_repo::move_subtree_in_tx(txn, req.id, req.parent_id, &new_parent_path, actor_id)
                .await?;
        updated = moved.ok_or_else(|| AppError::Biz("部门不存在".to_string()))?;
    }

    Ok(updated)
}

/// 事务内删除部门：有活子部门或有用户挂载引用时拒绝，否则软删。
pub async fn delete_dept_in_tx(
    txn: &DatabaseTransaction,
    dept_id: u64,
    actor_id: u64,
) -> Result<(), AppError> {
    if dept_repo::find_by_id(txn, dept_id).await?.is_none() {
        return Err(AppError::Biz("部门不存在".to_string()));
    }

    // 存在活子部门 → 拒绝（避免产生游离子树）
    if !dept_repo::find_children(txn, dept_id).await?.is_empty() {
        return Err(AppError::Biz("存在下级部门，无法删除".to_string()));
    }

    // 存在用户挂载引用 → 拒绝（保持组织归属完整性）
    if dept_repo::count_user_refs_by_dept_id(txn, dept_id).await? > 0 {
        return Err(AppError::Biz("部门下存在用户，无法删除".to_string()));
    }

    // 软删：命中 0 行说明并发下目标已被删除
    if !dept_repo::soft_delete_dept_in_tx(txn, dept_id, actor_id).await? {
        return Err(AppError::Biz("部门不存在".to_string()));
    }
    Ok(())
}

/// 按 id 查部门详情（软删视为不存在）。
pub async fn get_dept(
    db: &impl ConnectionTrait,
    dept_id: u64,
) -> Result<sys_dept::Model, AppError> {
    let Some(dept) = dept_repo::find_by_id(db, dept_id).await? else {
        return Err(AppError::Biz("部门不存在".to_string()));
    };
    Ok(dept)
}

/// 批量按 id 查有效部门（排除软删；停用部门仍返回），供跨域引用校验
/// （如 user 挂载部门存在性）与批量名称拼装。空入参返回空数组。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> Result<Vec<sys_dept::Model>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(dept_repo::find_by_ids(db, ids).await?)
}

/// 部门树列表：全量有效部门（含停用）按 `parent_id` 递归组装，返回根节点集合。
pub async fn list_dept_tree(db: &impl ConnectionTrait) -> Result<Vec<DeptResp>, AppError> {
    let all = dept_repo::find_all_active(db).await?;

    // 平铺 → 按 parent_id 分组（一次查询，无 N+1），再由根递归组装 children
    let mut by_parent: HashMap<u64, Vec<DeptResp>> = HashMap::new();
    for model in all {
        let resp = DeptResp::from(model);
        by_parent.entry(resp.parent_id).or_default().push(resp);
    }

    /// 递归组装：同层按 `sort`、`id` 升序；超过深度上限即截断（防脏数据成环）。
    fn build(
        parent_id: u64,
        depth: usize,
        by_parent: &mut HashMap<u64, Vec<DeptResp>>,
    ) -> Vec<DeptResp> {
        if depth > MAX_DEPT_TREE_DEPTH {
            return Vec::new();
        }
        let mut nodes = by_parent.remove(&parent_id).unwrap_or_default();
        nodes.sort_by(|a, b| a.sort.cmp(&b.sort).then(a.id.cmp(&b.id)));
        for node in &mut nodes {
            node.children = build(node.id, depth + 1, by_parent);
        }
        nodes
    }

    Ok(build(0, 1, &mut by_parent))
}

/// 对外入口：开事务后委托 `create_dept_in_tx`，成功后提交。
pub async fn create_dept(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_dept_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 对外入口：开事务后委托 `update_dept_in_tx`，成功后提交。
pub async fn update_dept(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_dept_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 对外入口：开事务后委托 `delete_dept_in_tx`，成功后提交。
pub async fn delete_dept(
    db: &DatabaseConnection,
    actor_id: u64,
    dept_id: u64,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_dept_in_tx(&txn, dept_id, actor_id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 批量填充部门树的审计人显示名（`created_by_name` / `updated_by_name`）。
///
/// 树无法走扁平的 `fill_user_names` 管道：这里递归收集全部人字段 id →
/// 一次批量查显示名 → 递归按 `UserRefNames` 协议应用（整体只查一次库）。
pub async fn fill_dept_audit_names(
    db: &impl ConnectionTrait,
    nodes: &mut [DeptResp],
) -> Result<(), AppError> {
    /// 递归收集审计人 id。
    fn collect(nodes: &[DeptResp], ids: &mut Vec<u64>) {
        for node in nodes {
            ids.push(node.created_by);
            ids.push(node.updated_by);
            collect(&node.children, ids);
        }
    }
    /// 递归应用名称映射。
    fn apply(nodes: &mut [DeptResp], names: &HashMap<u64, String>) {
        for node in nodes.iter_mut() {
            node.set_user_ref_names(names);
            apply(&mut node.children, names);
        }
    }

    let mut ids = Vec::new();
    collect(nodes, &mut ids);
    let names = find_user_name_map_by_ids(db, ids).await?;
    apply(nodes, &names);
    Ok(())
}

/// 批量填充部门树的负责人列表（`sys_user_dept.is_leader = 1`，一对一部门可多人）。
///
/// 一次查询覆盖整棵树：递归收集部门 id → 批量查关联与用户名 → 递归应用。
pub async fn fill_dept_leaders(
    db: &impl ConnectionTrait,
    nodes: &mut [DeptResp],
) -> Result<(), AppError> {
    /// 递归收集部门 id。
    fn collect_ids(nodes: &[DeptResp], ids: &mut Vec<u64>) {
        for node in nodes {
            ids.push(node.id);
            collect_ids(&node.children, ids);
        }
    }
    /// 递归应用负责人分组。
    fn apply(nodes: &mut [DeptResp], by_dept: &mut HashMap<u64, Vec<DeptLeader>>) {
        for node in nodes.iter_mut() {
            if let Some(leaders) = by_dept.remove(&node.id) {
                node.leaders = leaders;
            }
            apply(&mut node.children, by_dept);
        }
    }

    let mut dept_ids = Vec::new();
    collect_ids(nodes, &mut dept_ids);
    let rows = dept_repo::find_leaders_by_dept_ids(db, &dept_ids).await?;

    let mut by_dept: HashMap<u64, Vec<DeptLeader>> = HashMap::new();
    for (dept_id, user_id, user_name) in rows {
        by_dept
            .entry(dept_id)
            .or_default()
            .push(DeptLeader { user_id, user_name });
    }
    apply(nodes, &mut by_dept);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_user_dept;
    use crate::modules::system::dept::repo as dept_repo;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, Database, TransactionTrait};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    async fn test_db() -> sea_orm::DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> DatabaseTransaction {
        test_db().await.begin().await.unwrap()
    }

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn create_req(parent_id: u64, dept_name: &str) -> CreateDeptReq {
        CreateDeptReq {
            parent_id,
            dept_name: dept_name.to_string(),
            sort: 0,
            status: 1,
            allow_peer_read: 0,
            remark: String::new(),
        }
    }

    fn update_req(id: u64, parent_id: u64, dept_name: &str) -> UpdateDeptReq {
        UpdateDeptReq {
            id,
            parent_id,
            dept_name: dept_name.to_string(),
            sort: 0,
            status: 1,
            allow_peer_read: 0,
            remark: String::new(),
        }
    }

    async fn seed_dept(
        txn: &DatabaseTransaction,
        parent_id: u64,
        dept_name: &str,
    ) -> sys_dept::Model {
        create_dept_in_tx(txn, ACTOR_ID, &create_req(parent_id, dept_name))
            .await
            .unwrap()
    }

    /// 造一条用户-部门挂载引用（关系表无外键，用户 id 用高位序号即可）。
    async fn seed_user_dept_ref(txn: &DatabaseTransaction, dept_id: u64) {
        sys_user_dept::ActiveModel {
            user_id: Set(9_000_000 + SEQ.fetch_add(1, Ordering::Relaxed)),
            dept_id: Set(dept_id),
            ..Default::default()
        }
        .insert(txn)
        .await
        .unwrap();
    }

    /// 在树里按 id 深度优先找节点。
    fn find_node(nodes: &[DeptResp], id: u64) -> Option<&DeptResp> {
        for node in nodes {
            if node.id == id {
                return Some(node);
            }
            if let Some(found) = find_node(&node.children, id) {
                return Some(found);
            }
        }
        None
    }

    #[tokio::test]
    async fn create_dept_rejects_missing_parent() {
        let txn = test_txn().await;
        let res = create_dept_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(9_999_999_999, &unique("no_parent")),
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("上级部门")),
            "父部门不存在应返回业务错误: {res:?}"
        );
    }

    #[tokio::test]
    async fn create_dept_rejects_soft_deleted_parent() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("del_parent")).await;
        dept_repo::soft_delete_dept_in_tx(&txn, root.id, ACTOR_ID)
            .await
            .unwrap();

        let res =
            create_dept_in_tx(&txn, ACTOR_ID, &create_req(root.id, &unique("under_del"))).await;
        assert!(
            matches!(res, Err(AppError::Biz(_))),
            "父部门已软删应拒绝创建: {res:?}"
        );
    }

    #[tokio::test]
    async fn create_dept_rejects_duplicate_name_under_same_parent() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("dup_root")).await;
        let name = unique("dup_name");
        let _first = seed_dept(&txn, root.id, &name).await;

        let res = create_dept_in_tx(&txn, ACTOR_ID, &create_req(root.id, &name)).await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("已存在")),
            "同父同名应返回业务错误（而非数据库错误）: {res:?}"
        );
    }

    #[tokio::test]
    async fn create_dept_allows_root_with_placeholder_path() {
        let txn = test_txn().await;
        let root = create_dept_in_tx(&txn, ACTOR_ID, &create_req(0, &unique("root_ok")))
            .await
            .unwrap();
        assert_eq!(root.parent_id, 0);
        assert_eq!(root.dept_path, format!("/0/{}/", root.id));
    }

    #[tokio::test]
    async fn update_dept_rejects_move_to_self() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("self_root")).await;

        let res = update_dept_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(root.id, root.id, &root.dept_name),
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("自身")),
            "移动到自身应被防环拒绝: {res:?}"
        );
    }

    #[tokio::test]
    async fn update_dept_rejects_move_to_descendant() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("cycle_root")).await;
        let child = seed_dept(&txn, root.id, &unique("cycle_child")).await;

        let res = update_dept_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(root.id, child.id, &root.dept_name),
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("自身")),
            "移动到自身下级应被防环拒绝: {res:?}"
        );
    }

    #[tokio::test]
    async fn update_dept_rejects_missing_parent() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("miss_parent")).await;

        let res = update_dept_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(root.id, 9_999_999_999, &root.dept_name),
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Biz(_))),
            "新父不存在应拒绝更新: {res:?}"
        );
    }

    #[tokio::test]
    // 端到端：service 校验通过后委托 repo 移动，整棵子树 path 重算。
    async fn update_dept_moves_subtree_and_recomputes_paths() {
        let txn = test_txn().await;
        let root_a = seed_dept(&txn, 0, &unique("mv_a")).await;
        let root_b = seed_dept(&txn, 0, &unique("mv_b")).await;
        let dept = seed_dept(&txn, root_a.id, &unique("mv_dept")).await;
        let grand = seed_dept(&txn, dept.id, &unique("mv_grand")).await;

        let moved = update_dept_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(dept.id, root_b.id, &dept.dept_name),
        )
        .await
        .unwrap();

        let expect_dept_path = format!("{}{}/", root_b.dept_path, dept.id);
        assert_eq!(moved.parent_id, root_b.id);
        assert_eq!(moved.dept_path, expect_dept_path);

        let reloaded_grand = dept_repo::find_by_id(&txn, grand.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reloaded_grand.dept_path,
            format!("{}{}/", expect_dept_path, grand.id),
            "孙节点 path 应随移动重算"
        );
    }

    #[tokio::test]
    // 不改父时只更新字段：parent_id 与 dept_path 保持不变。
    async fn update_dept_updates_fields_without_move() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("field_root")).await;

        let mut req = update_req(root.id, 0, &format!("{}_v2", root.dept_name));
        req.allow_peer_read = 1;
        req.status = 0;
        req.remark = "组织调整".to_string();
        let updated = update_dept_in_tx(&txn, ACTOR_ID, &req).await.unwrap();

        assert_eq!(updated.dept_name, format!("{}_v2", root.dept_name));
        assert_eq!(updated.allow_peer_read, 1);
        assert_eq!(updated.status, 0);
        assert_eq!(updated.remark, "组织调整");
        assert_eq!(updated.parent_id, 0);
        assert_eq!(updated.dept_path, root.dept_path, "未移动时 path 不应变化");
    }

    #[tokio::test]
    async fn delete_dept_rejects_when_has_active_children() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("del_root")).await;
        let _child = seed_dept(&txn, root.id, &unique("del_child")).await;

        let res = delete_dept_in_tx(&txn, root.id, ACTOR_ID).await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("下级部门")),
            "存在活子部门应拒绝删除: {res:?}"
        );
        assert!(
            dept_repo::find_by_id(&txn, root.id)
                .await
                .unwrap()
                .is_some(),
            "被拒绝删除的部门应仍然存在"
        );
    }

    #[tokio::test]
    async fn delete_dept_rejects_when_user_referenced() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("ref_root")).await;
        seed_user_dept_ref(&txn, root.id).await;

        let res = delete_dept_in_tx(&txn, root.id, ACTOR_ID).await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("用户")),
            "存在用户挂载应拒绝删除: {res:?}"
        );
        assert!(
            dept_repo::find_by_id(&txn, root.id)
                .await
                .unwrap()
                .is_some(),
            "被拒绝删除的部门应仍然存在"
        );
    }

    #[tokio::test]
    async fn delete_dept_soft_deletes_leaf() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("leaf_root")).await;
        let leaf = seed_dept(&txn, root.id, &unique("leaf")).await;

        delete_dept_in_tx(&txn, leaf.id, ACTOR_ID).await.unwrap();

        assert!(
            dept_repo::find_by_id(&txn, leaf.id)
                .await
                .unwrap()
                .is_none(),
            "删除后应查不到该部门"
        );
        assert!(
            dept_repo::find_by_id(&txn, root.id)
                .await
                .unwrap()
                .is_some(),
            "父部门不应受影响"
        );
    }

    #[tokio::test]
    async fn get_dept_rejects_soft_deleted() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("get_root")).await;
        dept_repo::soft_delete_dept_in_tx(&txn, root.id, ACTOR_ID)
            .await
            .unwrap();

        let res = get_dept(&txn, root.id).await;
        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("不存在")),
            "软删部门详情应视为不存在: {res:?}"
        );
    }

    #[tokio::test]
    // 树组装：子节点挂到父下，且停用节点（status=0）也保留在树中。
    async fn list_dept_tree_builds_nested_children_including_disabled() {
        let txn = test_txn().await;
        let root = seed_dept(&txn, 0, &unique("tree_root")).await;
        let live = seed_dept(&txn, root.id, &unique("tree_live")).await;
        let disabled = seed_dept(&txn, root.id, &unique("tree_disabled")).await;
        let mut req = update_req(disabled.id, root.id, &disabled.dept_name);
        req.status = 0;
        update_dept_in_tx(&txn, ACTOR_ID, &req).await.unwrap();

        let tree = list_dept_tree(&txn).await.unwrap();
        let root_node = find_node(&tree, root.id).expect("根节点应出现在树中");
        assert_eq!(root_node.children.len(), 2, "两个子部门都应挂到根下");

        let disabled_node = find_node(&tree, disabled.id).expect("停用部门也应保留在树中");
        assert_eq!(disabled_node.status, 0);
        assert_eq!(disabled_node.parent_id, root.id);
        assert!(find_node(&tree, live.id).is_some());
    }
}
