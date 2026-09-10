//! 部门业务编排层：树规则（父存在性 / 防环 / 同父同名 / 删除占用）与树组装。
//!
//! `*_in_tx` 不管理事务边界：由 api 入口 begin/commit，测试用外层事务包裹。
//! 树组装在内存完成（一次全量查询 + HashMap 递归），不做 N+1 查询。

use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseTransaction};

use crate::entity::sys_dept;
use crate::modules::dept::dto::{CreateDeptReq, DeptResp, UpdateDeptReq};
use crate::modules::dept::repo as dept_repo;
use crate::utils::error::AppError;

/// 事务内创建部门：父存在性 + 同父同名校验后委托 repo 落库（含 path 回写）。
///
/// 规则：
/// - `parent_id = 0` 为根部门，path 占位 `/0/`；
/// - 父部门不存在或已软删 → `AppError::Biz`；
/// - 同父下部门名重复（含软删占位）→ `AppError::Biz`；
/// - `parent_path` 由本层组装/校验（新父 `dept_path`，根传 `/0/`），保证以 `/` 结尾。
pub async fn create_dept_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    let dept = dept_repo::find_by_name_include_deleted(txn, &req.dept_name).await?;
    if dept.is_none() {
        return Err(AppError::Biz("部门名已存在".to_string()));
    }

    let Some(parent_dept) = dept_repo::find_by_id(txn, req.parent_id).await? else {
        return Err(AppError::Biz("父部门不存在".to_string()));
    };

    let model = dept_repo::create_dept_in_tx(
        txn,
        sys_dept::ActiveModel {
            dept_name: Set(req.dept_name.clone()),
            parent_id: Set(req.parent_id),
            sort: Set(req.sort),
            status: Set(req.status),
            allow_peer_read: Set(req.allow_peer_read),
            ..Default::default()
        },
        &parent_dept.dept_path,
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
/// - 组装移动入参前校验 `new_parent_path` 以 `/` 结尾（新父 `dept_path` 天然满足，
///   根部门传占位 `/0/`）——该入参约定由本层保证，repo 不做判断。
pub async fn update_dept_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    todo!(
        "service 实现：目标/新父校验 + 防环 + 同名查重 + new_parent_path 约定校验 → repo 字段更新或 move_subtree_in_tx"
    )
}

/// 事务内删除部门：有活子部门或有用户挂载引用时拒绝，否则软删。
pub async fn delete_dept_in_tx(txn: &DatabaseTransaction, dept_id: u64) -> Result<(), AppError> {
    todo!("service 实现：子部门检查 + count_user_refs_by_dept_id 占用检查 → soft_delete_dept_in_tx")
}

/// 按 id 查部门详情（软删视为不存在）。
pub async fn get_dept(
    db: &impl ConnectionTrait,
    dept_id: u64,
) -> Result<sys_dept::Model, AppError> {
    todo!("service 实现：dept_repo::find_by_id，None → AppError::Biz(\"部门不存在\")")
}

/// 部门树列表：全量有效部门（含停用）按 `parent_id` 递归组装，返回根节点集合。
pub async fn list_dept_tree(db: &impl ConnectionTrait) -> Result<Vec<DeptResp>, AppError> {
    todo!("service 实现：dept_repo::find_all_active → HashMap 分组递归组装 children")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_user_dept;
    use crate::modules::dept::repo as dept_repo;
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
    fn find_node<'a>(nodes: &'a [DeptResp], id: u64) -> Option<&'a DeptResp> {
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

        let res = delete_dept_in_tx(&txn, root.id).await;
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

        let res = delete_dept_in_tx(&txn, root.id).await;
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

        delete_dept_in_tx(&txn, leaf.id).await.unwrap();

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
