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
/// - 父行走**加锁读**：既取最新 `dept_path`（普通读在 RR 下走快照，并发移动父会拼出
///   陈旧 path），又与并发移动/删除该父的事务在父行上互斥。
pub async fn create_dept_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    // 父部门校验：parent_id = 0 为根部门（无父），否则须存在且未软删
    let parent_path = if req.parent_id == 0 {
        ROOT_PARENT_PATH.to_string()
    } else {
        let Some(parent) = dept_repo::find_by_id_for_update(txn, req.parent_id).await? else {
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

/// 移动前的行锁：按 id 升序对「被移动节点」与「新父」加排他锁，返回两者最新记录。
///
/// 防环判定依赖「读取对方最新的 `dept_path`」，而 RR 下普通读走事务快照——两个并发
/// 互移请求（`A 移到 B 下` 与 `B 移到 A 下`）各自只看到旧结构，会双双通过校验、双双
/// 提交，最终互指成环（两部门从树中消失，`dept_path` 也被拼坏）。加锁读同时给出
/// 新鲜度（读最新已提交版本）与互斥（两事务在同样的行上串行化），使其中一方必被拒绝。
///
/// 升序加锁是防死锁的关键：方向相反的互移请求加锁顺序一致，不会交叉等待；若按
/// 「先自身后新父」的顺序加锁，两个请求就会互等并触发 `Deadlock found`。
///
/// 新父 `id = 0` 表示移到根，无父行可锁。返回值中 `None` 表示目标行不存在（或已软删），
/// 「部门不存在 / 上级部门不存在」的文案由调用方决定。
async fn lock_move_rows(
    txn: &DatabaseTransaction,
    dept_id: u64,
    parent_id: u64,
) -> Result<(Option<sys_dept::Model>, Option<sys_dept::Model>), AppError> {
    // 移到根：只有被移动节点自身需要加锁
    if parent_id == 0 {
        let dept = dept_repo::find_by_id_for_update(txn, dept_id).await?;
        return Ok((dept, None));
    }

    // 小 id 先锁（自环场景 parent_id == dept_id 是同一行，重复加锁无副作用）
    if parent_id <= dept_id {
        let parent = dept_repo::find_by_id_for_update(txn, parent_id).await?;
        let dept = dept_repo::find_by_id_for_update(txn, dept_id).await?;
        Ok((dept, parent))
    } else {
        let dept = dept_repo::find_by_id_for_update(txn, dept_id).await?;
        let parent = dept_repo::find_by_id_for_update(txn, parent_id).await?;
        Ok((dept, parent))
    }
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
/// - 目标部门与新父一律**加锁读**校验（`lock_move_rows`）：防环是「读-判-写」不变式，
///   快照读会被并发互移绕过成环。
pub async fn update_dept_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateDeptReq,
) -> Result<sys_dept::Model, AppError> {
    let (dept, parent) = lock_move_rows(txn, req.id, req.parent_id).await?;

    let Some(dept) = dept else {
        return Err(AppError::Biz("部门不存在".to_string()));
    };

    // 新父校验：parent_id = 0 为根；否则须存在未软删，且不得为自身或自身下级（防环）
    let new_parent_path = if req.parent_id == 0 {
        ROOT_PARENT_PATH.to_string()
    } else {
        let Some(parent) = parent else {
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
///
/// 三处判定全部走**加锁读**，顺序固定为「自身行 → 子部门区间 → 引用区间」：
/// - 自身行：与并发「在该部门下建子 / 把节点移入本部门 / 删除本部门」在自身行上互斥；
/// - 子部门区间：与并发在该父下插入子部门的插入意向锁互斥；
/// - 引用区间：与并发挂载用户（user 域插入 `sys_user_dept`）的插入意向锁互斥。
///
/// 普通读在 RR 下走事务快照，三类并发都可能被漏判，进而删出游离子树或产生悬挂引用。
pub async fn delete_dept_in_tx(
    txn: &DatabaseTransaction,
    dept_id: u64,
    actor_id: u64,
) -> Result<(), AppError> {
    if dept_repo::find_by_id_for_update(txn, dept_id)
        .await?
        .is_none()
    {
        return Err(AppError::Biz("部门不存在".to_string()));
    }

    // 存在活子部门 → 拒绝（避免产生游离子树）
    if !dept_repo::find_children_for_update(txn, dept_id)
        .await?
        .is_empty()
    {
        return Err(AppError::Biz("存在下级部门，无法删除".to_string()));
    }

    // 存在用户挂载引用 → 拒绝（保持组织归属完整性）
    if !dept_repo::find_user_refs_by_dept_id_for_update(txn, dept_id)
        .await?
        .is_empty()
    {
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

/// 批量按 id 查有效部门并加锁（挂载引用前校验用），空入参返回空数组。
///
/// 与 `find_by_ids` 的区别是**加了排他锁**：跨域写入（user 域写 `sys_user_dept`）必须先
/// 锁住被引用的部门行，才能与「删除部门」的占用检查串行化——删除方检查完引用后挂载方
/// 才插入，会留下指向已软删部门的悬挂引用。须在事务内调用。
pub async fn find_by_ids_for_update(
    txn: &DatabaseTransaction,
    ids: &[u64],
) -> Result<Vec<sys_dept::Model>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(dept_repo::find_by_ids_for_update(txn, ids).await?)
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
    use sea_orm::{ActiveModelTrait, Database};
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

    /// 硬删已提交的测试数据。并发用例的两个事务必须**真实提交**才能复现写偏斜，
    /// 无法复用 `test_txn()` 的回滚隔离，只能用例内手工清理。
    async fn hard_delete_depts(db: &DatabaseConnection, ids: &[u64]) {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        sys_dept::Entity::delete_many()
            .filter(sys_dept::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
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

    /// 硬删测试插入的用户-部门挂载引用（关系表硬删，按 dept_id 清理）。
    async fn hard_delete_user_depts(db: &DatabaseConnection, dept_id: u64) {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        sys_user_dept::Entity::delete_many()
            .filter(sys_user_dept::Column::DeptId.eq(dept_id))
            .exec(db)
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

    /// 并发互移防环：事务 1「A 移到 B 下」与事务 2「B 移到 A 下」各自校验时都看不到
    /// 对方的未提交改动（RR 快照读），两边都放行即互指成环，两部门从树中消失。
    ///
    /// 复现要点：事务 2 先开始并读一次以固定快照，事务 1 完整提交后事务 2 才继续——
    /// 这正是两个真实并发请求的交错顺序，且不依赖线程调度，结果确定。
    #[tokio::test]
    async fn update_dept_concurrent_cross_move_cannot_create_cycle() {
        let db = test_db().await;
        let root = create_dept(&db, ACTOR_ID, &create_req(0, &unique("race_root")))
            .await
            .unwrap();
        let a = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("race_a")))
            .await
            .unwrap();
        let b = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("race_b")))
            .await
            .unwrap();

        // 事务 2 先开启并做一次快照读（此后它的所有普通读都停留在这一刻）
        let txn2 = db.begin().await.unwrap();
        assert!(
            dept_repo::find_by_id(&txn2, a.id).await.unwrap().is_some(),
            "前置：A 应存在"
        );

        // 事务 1：A 移到 B 下，提交
        let txn1 = db.begin().await.unwrap();
        let moved_a =
            update_dept_in_tx(&txn1, ACTOR_ID, &update_req(a.id, b.id, &a.dept_name)).await;
        assert!(moved_a.is_ok(), "前置：单向移动应放行: {moved_a:?}");
        txn1.commit().await.unwrap();

        // 事务 2：B 移到 A 下——A 已是 B 的父，必须被防环拒绝
        let res = update_dept_in_tx(&txn2, ACTOR_ID, &update_req(b.id, a.id, &b.dept_name)).await;
        if res.is_ok() {
            txn2.commit().await.unwrap();
        } else {
            txn2.rollback().await.unwrap();
        }

        // 用户可见症状：成环后两个部门都不再挂在根下，整棵树里消失
        let tree = list_dept_tree(&db).await.unwrap();
        let reachable = (
            find_node(&tree, a.id).is_some(),
            find_node(&tree, b.id).is_some(),
        );
        hard_delete_depts(&db, &[a.id, b.id, root.id]).await;

        assert!(
            res.is_err(),
            "并发互移必须拒绝其一，否则 A/B 互指成环: {res:?}"
        );
        assert_eq!(
            reachable,
            (true, true),
            "A/B 都应仍能从根遍历到（成环则双双从树中消失）"
        );
    }

    /// 并发「建子部门」+「移动其父」：移动方必须看到已提交的新建子节点，否则该子节点的
    /// `dept_path` 停留在旧父链（与真实祖先链不符，进而让基于 path 前缀的防环判定失效）。
    ///
    /// 复现要点同并发互移用例：事务 2 先读一次固定快照，事务 1 建子并提交后事务 2 才移动。
    #[tokio::test]
    async fn move_dept_recomputes_child_created_after_snapshot() {
        let db = test_db().await;
        let root = create_dept(&db, ACTOR_ID, &create_req(0, &unique("stale_root")))
            .await
            .unwrap();
        let parent = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("stale_parent")))
            .await
            .unwrap();
        let other = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("stale_other")))
            .await
            .unwrap();

        // 事务 2 先固定快照（此后它的普通读停在「建子之前」）
        let txn2 = db.begin().await.unwrap();
        dept_repo::find_by_id(&txn2, parent.id)
            .await
            .unwrap()
            .expect("前置：父部门应存在");

        // 事务 1：在父部门下建子，提交
        let txn1 = db.begin().await.unwrap();
        let child = create_dept_in_tx(
            &txn1,
            ACTOR_ID,
            &create_req(parent.id, &unique("stale_child")),
        )
        .await
        .unwrap();
        txn1.commit().await.unwrap();

        // 事务 2：把父移到 other 下（子树 path 需按新父链重算，含刚建的子）
        let moved = update_dept_in_tx(
            &txn2,
            ACTOR_ID,
            &update_req(parent.id, other.id, &parent.dept_name),
        )
        .await
        .expect("移动到无环位置应放行");
        txn2.commit().await.unwrap();

        let reloaded_child = dept_repo::find_by_id(&db, child.id)
            .await
            .unwrap()
            .expect("子部门应存在");
        hard_delete_depts(&db, &[child.id, parent.id, other.id, root.id]).await;

        assert_eq!(
            reloaded_child.dept_path,
            format!("{}{}/", moved.dept_path, child.id),
            "新建子节点的 dept_path 应随父移动重算，而不是停留在旧父链"
        );
    }

    /// 并发「移动父」+「在其下建子」：建子方必须读到移动后的最新父 `dept_path`。
    ///
    /// 与上一个用例互补：这里移动方先发起并**持有父行锁**（尚未提交），建子方随后插入。
    /// 若建子方用普通读，它不会被阻塞、也不会看到未提交的移动，会按旧父链拼出陈旧 path
    /// ——加锁读的互斥只挡得住插入时机，**新鲜度仍需建子方自己加锁读父行**。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_dept_under_parent_moved_concurrently_uses_new_path() {
        let db = test_db().await;
        let root = create_dept(&db, ACTOR_ID, &create_req(0, &unique("cm_root")))
            .await
            .unwrap();
        let parent = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("cm_parent")))
            .await
            .unwrap();
        let other = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("cm_other")))
            .await
            .unwrap();

        // 移动方：先改父 + 重算子树（未提交，父行锁与父的索引间隙仍在手里）
        let txn_move = db.begin().await.unwrap();
        let moved = update_dept_in_tx(
            &txn_move,
            ACTOR_ID,
            &update_req(parent.id, other.id, &parent.dept_name),
        )
        .await
        .expect("移动到无环位置应放行");

        // 建子方：另一条连接上发起，卡在移动方持有的父行锁上
        let db_create = db.clone();
        let (parent_id, child_name) = (parent.id, unique("cm_child"));
        let handle = tokio::spawn(async move {
            create_dept(&db_create, ACTOR_ID, &create_req(parent_id, &child_name)).await
        });
        // 让建子方抵达锁等待，再提交移动方（建子若已提前完成，本用例会失败在下面的断言上）
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        txn_move.commit().await.unwrap();

        let created = handle.await.unwrap().expect("建子应成功");
        hard_delete_depts(&db, &[created.id, parent.id, other.id, root.id]).await;

        assert_eq!(
            created.dept_path,
            format!("{}{}/", moved.dept_path, created.id),
            "并发下新建的子部门应挂到移动后的新父链上"
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

    /// 并发「建子部门」+「删父部门」：删除方必须看到已提交的新建子部门，否则删出游离子树。
    ///
    /// 复现要点同并发互移用例：事务 2 先读一次固定快照，事务 1 建子并提交后事务 2 才删除。
    #[tokio::test]
    async fn delete_dept_rejects_child_created_after_snapshot() {
        let db = test_db().await;
        let root = create_dept(&db, ACTOR_ID, &create_req(0, &unique("dc_root")))
            .await
            .unwrap();
        let parent = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("dc_parent")))
            .await
            .unwrap();

        // 事务 2 先固定快照（此后它的普通读停在「建子之前」）
        let txn2 = db.begin().await.unwrap();
        dept_repo::find_by_id(&txn2, parent.id)
            .await
            .unwrap()
            .expect("前置：父部门应存在");

        // 事务 1：在父部门下建子，提交
        let txn1 = db.begin().await.unwrap();
        let child = create_dept_in_tx(&txn1, ACTOR_ID, &create_req(parent.id, &unique("dc_child")))
            .await
            .unwrap();
        txn1.commit().await.unwrap();

        let res = delete_dept_in_tx(&txn2, parent.id, ACTOR_ID).await;
        // 必须先结束事务再清理：txn2 被判拒绝后仍持有父行锁，否则清理的硬删会被锁等待挡住
        txn2.rollback().await.unwrap();
        let parent_alive = dept_repo::find_by_id(&db, parent.id)
            .await
            .unwrap()
            .is_some();
        hard_delete_depts(&db, &[child.id, parent.id, root.id]).await;

        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("下级部门")),
            "并发新建的子部门应挡住删除: {res:?}"
        );
        assert!(
            parent_alive,
            "被拒后父部门不应被软删（否则子部门成游离子树）"
        );
    }

    /// 并发「挂载用户到部门」+「删部门」：删除方必须看到已提交的挂载引用。
    #[tokio::test]
    async fn delete_dept_rejects_user_ref_inserted_after_snapshot() {
        let db = test_db().await;
        let root = create_dept(&db, ACTOR_ID, &create_req(0, &unique("dr_root")))
            .await
            .unwrap();
        let parent = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("dr_parent")))
            .await
            .unwrap();

        // 事务 2 先固定快照（此后它的普通读停在「挂载之前」）
        let txn2 = db.begin().await.unwrap();
        dept_repo::find_by_id(&txn2, parent.id)
            .await
            .unwrap()
            .expect("前置：父部门应存在");

        // 事务 1：插入用户挂载引用，提交
        let txn1 = db.begin().await.unwrap();
        seed_user_dept_ref(&txn1, parent.id).await;
        txn1.commit().await.unwrap();

        let res = delete_dept_in_tx(&txn2, parent.id, ACTOR_ID).await;
        // 必须先结束事务再清理：txn2 被判拒绝后仍持有父行锁与引用区间锁
        txn2.rollback().await.unwrap();
        let parent_alive = dept_repo::find_by_id(&db, parent.id)
            .await
            .unwrap()
            .is_some();
        hard_delete_user_depts(&db, parent.id).await;
        hard_delete_depts(&db, &[parent.id, root.id]).await;

        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("用户")),
            "并发插入的挂载引用应挡住删除: {res:?}"
        );
        assert!(
            parent_alive,
            "被拒后父部门不应被软删（否则引用指向已删部门）"
        );
    }

    /// 真并发冒烟：删部门 与 在其下建子部门 不能同时成功，且不得留下游离子树。
    ///
    /// 只断言与调度无关的不变量（两者不可同时成功；不存在「父已软删但仍有活子部门」）；
    /// 哪一方胜出由抢锁顺序决定，不做断言。数据真实提交（并发必需），用真连接 + 手工清理。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_create_child_and_delete_parent_leaves_no_orphan_subtree() {
        let db = test_db().await;
        let parent = create_dept(&db, ACTOR_ID, &create_req(0, &unique("orphan_parent")))
            .await
            .unwrap();
        let parent_id = parent.id;

        let (db_delete, db_create) = (db.clone(), db.clone());
        let child_name = unique("orphan_child");
        let delete_handle =
            tokio::spawn(async move { delete_dept(&db_delete, ACTOR_ID, parent_id).await });
        let create_handle = tokio::spawn(async move {
            create_dept(&db_create, ACTOR_ID, &create_req(parent_id, &child_name)).await
        });
        let (deleted, created) = (delete_handle.await.unwrap(), create_handle.await.unwrap());

        // 先采集状态并清理，再断言（避免断言 panic 留下孤儿数据）
        let parent_alive = dept_repo::find_by_id(&db, parent_id)
            .await
            .unwrap()
            .is_some();
        // 建子成功时返回模型即子部门 id，无需再查一次
        let created_child_id = created.as_ref().ok().map(|child| child.id);
        if let Some(child_id) = created_child_id {
            hard_delete_depts(&db, &[child_id]).await;
        }
        hard_delete_depts(&db, &[parent_id]).await;

        assert!(
            !(deleted.is_ok() && created.is_ok()),
            "删除与建子不应同时成功: deleted={deleted:?} created={created:?}"
        );
        assert!(
            parent_alive || created_child_id.is_none(),
            "不应留下游离子树（父已软删但子部门仍在）"
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

    /// 真并发冒烟：两个线程同时发起方向相反的互移，验证按 id 升序加锁不会交叉等待。
    ///
    /// 与上一个用例的分工：上一个用「先固定快照再串行」确定性复现写偏斜本身；
    /// 本用例跑真并发，覆盖**加锁顺序**——若改成「先自身后新父」的顺序加锁，这里会
    /// 触发 `Deadlock found`（1213）或锁等待超时（1205）。
    ///
    /// 只断言与调度无关的不变量（至少一方被拒、树中无环）；具体哪一方胜出由抢锁顺序
    /// 决定，不做断言。数据真实提交（并发必需），故用真连接 + 手工清理。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_cross_moves_reject_one_side_without_deadlock() {
        let db = test_db().await;
        let root = create_dept(&db, ACTOR_ID, &create_req(0, &unique("smoke_root")))
            .await
            .unwrap();
        let a = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("smoke_a")))
            .await
            .unwrap();
        let b = create_dept(&db, ACTOR_ID, &create_req(root.id, &unique("smoke_b")))
            .await
            .unwrap();

        // 两条连接各自开事务，模拟两个并发请求：A 移到 B 下 / B 移到 A 下
        let (db1, db2) = (db.clone(), db.clone());
        let (req1, req2) = (
            update_req(a.id, b.id, &a.dept_name),
            update_req(b.id, a.id, &b.dept_name),
        );
        let h1 = tokio::spawn(async move { update_dept(&db1, ACTOR_ID, &req1).await });
        let h2 = tokio::spawn(async move { update_dept(&db2, ACTOR_ID, &req2).await });
        let (r1, r2) = (h1.await.unwrap(), h2.await.unwrap());

        let tree = list_dept_tree(&db).await.unwrap();
        let reachable = (
            find_node(&tree, a.id).is_some(),
            find_node(&tree, b.id).is_some(),
        );
        hard_delete_depts(&db, &[a.id, b.id, root.id]).await;

        assert!(
            r1.is_err() || r2.is_err(),
            "并发互移至少一方应被防环拒绝: r1={r1:?} r2={r2:?}"
        );
        assert_eq!(reachable, (true, true), "不应成环");
    }

    /// MySQL 死锁（1213）应被映射成可重试的业务文案，而不是 `Internal` 的固定串
    /// （覆盖 `utils::error` 的 `From<anyhow::Error>` 分支）。
    ///
    /// 必须真库：`MySqlDatabaseError` 无公开构造，纯单测造不出带错误号的 `DbErr`，
    /// 故 `utils::error` 的单测只覆盖反例。这里用两条连接交叉加锁触发 InnoDB 死锁检测。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn db_deadlock_maps_to_retryable_biz_error() {
        let db = test_db().await;
        let a = create_dept(&db, ACTOR_ID, &create_req(0, &unique("deadlock_a")))
            .await
            .unwrap();
        let b = create_dept(&db, ACTOR_ID, &create_req(0, &unique("deadlock_b")))
            .await
            .unwrap();

        let txn1 = db.begin().await.unwrap();
        let txn2 = db.begin().await.unwrap();
        // 各自先占一行，再互等对方持有的行 → InnoDB 立刻判定死锁并回滚其中一方
        dept_repo::find_by_id_for_update(&txn1, a.id).await.unwrap();
        dept_repo::find_by_id_for_update(&txn2, b.id).await.unwrap();
        let (r1, r2) = tokio::join!(
            dept_repo::find_by_id_for_update(&txn1, b.id),
            dept_repo::find_by_id_for_update(&txn2, a.id),
        );
        let _ = txn1.rollback().await;
        let _ = txn2.rollback().await;
        hard_delete_depts(&db, &[a.id, b.id]).await;

        let loser = [r1, r2]
            .into_iter()
            .find(|result| result.is_err())
            .expect("交叉加锁必然产生死锁，其中一方应报错");
        let mapped = AppError::from(loser.unwrap_err());
        assert!(
            matches!(mapped, AppError::Biz(ref m) if m.contains("重试")),
            "死锁应映射为可重试业务文案: {mapped:?}"
        );
    }
}
