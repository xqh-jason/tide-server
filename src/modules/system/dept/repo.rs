//! 部门数据访问原语。
//!
//! `dept_path` 规范：段为从根占位 `0` 到**自身**的整条部门 id 链。
//! - 根部门（`parent_id = 0`）：`/0/{自身id}/`；
//! - 子部门：`{父部门.path}{自身id}/`，即 `dept_path = parent_path + id + "/"`。
//!
//! 移动节点后整棵子树 path 需按新父链重算（见 `move_subtree_in_tx`）。
//!
//! 写原语遵循 `*_in_tx` 模式：不自行 begin/commit，边界由 service 入口与
//! 测试外层事务负责；审计字段由 repo 统一盖章。

use std::collections::{HashMap, HashSet};

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{DatabaseTransaction, QueryOrder, QuerySelect};

use crate::entity::{sys_dept, sys_user, sys_user_dept};

/// 事务内创建部门：审计盖章 + 插入 + 回写 `dept_path`。
///
/// - `parent_path`：父部门 path；根部门（`parent_id = 0`）传占位 `/0/`。
/// - 回写规则：`dept_path = format!("{parent_path}{id}/")`。
pub async fn create_dept_in_tx(
    txn: &DatabaseTransaction,
    dept: sys_dept::ActiveModel,
    parent_path: &str,
    actor_id: u64,
) -> anyhow::Result<sys_dept::Model> {
    // 审计盖章：创建时创建人与更新人同源
    let mut dept = dept;
    dept.created_by = Set(actor_id);
    dept.updated_by = Set(actor_id);

    // 先插入拿到自增 id（dept_path 走 DB 默认空串占位，下一步回写）
    let model = dept.insert(txn).await?;

    // 回写 dept_path：只 Set 主键 + dept_path 两列（窄写，避免全列覆盖写）
    let dept_path = format!("{}{}/", parent_path, model.id);
    let active_model = sys_dept::ActiveModel {
        id: Set(model.id),
        dept_path: Set(dept_path.clone()),
        ..Default::default()
    };
    active_model.update(txn).await?;

    // 用内存值组装返回，省一次查询：insert 结果已是完整行，仅 path 为回写后的新值
    let mut created = model;
    created.dept_path = dept_path;
    Ok(created)
}

/// 事务内普通字段更新（不含换父，改名/改开关等）：仅刷新 `updated_by`。
/// parent_id 变更请走 `move_subtree_in_tx`。
///
/// 入参 `dept` 由调用方构造：**只 Set 需要变更的列**（如 `dept_name`、
/// `allow_peer_read`、`remark` 等），其余列留 `NotSet`，避免全列覆盖写。
pub async fn update_dept_in_tx(
    txn: &DatabaseTransaction,
    dept: sys_dept::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<sys_dept::Model> {
    let mut dept = dept;
    dept.updated_by = Set(actor_id);
    // 更新数据库
    let updated_model = dept.update(txn).await?;
    Ok(updated_model)
}

/// 事务内移动部门：更新 `parent_id` 并按新父链重算**该节点及其整棵子树**的
/// `dept_path`（子部门 path = 父新 path + 自身 id + `/`，逐层下推）。
///
/// # 入参约定
/// - `new_parent_id = 0` 表示移到根，此时 `new_parent_path` 传根占位 `/0/`；
///   非根时传**新父当前**的 `dept_path`。
/// - `new_parent_path` 必须以 `/` 结尾（`debug_assert` 校验，防漏写尾斜杠拼出
///   `/0/58/` 这类静默脏路径）。
///
/// # 读一致性（加锁读）
/// 被移动节点与逐层下推的子节点查询都走 `SELECT ... FOR UPDATE`：普通读在 RR 下走事务
/// 快照，会漏掉并发事务已提交的新建子节点，使这些子节点停留在旧 `dept_path`（与真实
/// 祖先链不符，进而让基于 path 前缀的防环判定失效）。加锁读还使本函数与并发
/// 「在该子树下建子节点」的插入意向锁互斥。
///
/// # 职责边界
/// 防环（新父不得为自身或自身下级）由 service 层负责；本函数额外用 `visited`
/// 作为防御性保险——单父模型 + 起点先改父的正常数据下不会触发，但若将来扩展
/// （多父、特殊下推规则）或库内出现异常脏数据，它能保证遍历必然终止而不是挂死。
///
/// # 审计
/// 被移动节点与全部后代的 `updated_by` 统一盖 `actor_id`；每行**只写变更列**
/// （`dept_path`，根节点另加 `parent_id`，加 `updated_by` 与主键 `id`），其余列
/// 保持 `NotSet` 不参与 UPDATE，避免全列覆盖写与读-改-写丢更新。
///
/// 目标部门不存在时返回 `Ok(None)`（业务错误文案由 service 层决定）；
/// 父未变更时直接返回原记录（no-op）；返回重算后被移动根节点的最新记录。
///
/// `new_parent_path` 必须是新父的 `dept_path`（根传占位 `/0/`，均以 `/` 结尾）：
/// 该入参约定由 **service 层在调用前校验/组装**，repo 不做入参判断。
pub async fn move_subtree_in_tx(
    txn: &DatabaseTransaction,
    dept_id: u64,
    new_parent_id: u64,
    new_parent_path: &str,
    actor_id: u64,
) -> anyhow::Result<Option<sys_dept::Model>> {
    let Some(dept) = find_by_id_for_update(txn, dept_id).await? else {
        return Ok(None);
    };

    // 父未变更 = 无需移动：直接返回，避免整树重算与无意义的 updated_at 刷新
    if dept.parent_id == new_parent_id {
        return Ok(Some(dept));
    }

    // 被移动节点自身：换父 + 重算自身 path；只 Set 变更列（parent_id/dept_path/updated_by）
    let self_path = format!("{new_parent_path}{dept_id}/");
    let dept_model = sys_dept::ActiveModel {
        id: Set(dept_id),
        parent_id: Set(new_parent_id),
        dept_path: Set(self_path.clone()),
        updated_by: Set(actor_id),
        ..Default::default()
    };
    dept_model.update(txn).await?;
    // 重新读取，返回完整最新记录（含 DB 侧 ON UPDATE 刷新的 updated_at）
    let Some(updated_model) = find_by_id(txn, dept_id).await? else {
        return Ok(None);
    };

    // 逐层下推：current 保存本层节点的 (id, 新 path)，下一层用本层的新 path 作前缀
    let mut current: Vec<(u64, String)> = vec![(dept_id, self_path)];
    // 防御性 visited：已访问节点不再入队，保证遍历必然终止
    let mut visited: HashSet<u64> = HashSet::from([dept_id]);
    while !current.is_empty() {
        let parent_ids: Vec<u64> = current.iter().map(|(id, _)| *id).collect();
        // 一次查出本层全部子节点（加锁读：见上方「读一致性」，兼作与并发建子节点的互斥点）
        let children = sys_dept::Entity::find()
            .filter(sys_dept::Column::ParentId.is_in(parent_ids))
            .lock_exclusive()
            .all(txn)
            .await?;
        // parent_id -> 父节点新 path 映射（本层节点）
        let path_map: HashMap<u64, String> = current.iter().cloned().collect();
        let mut next = Vec::with_capacity(children.len());
        for child in children {
            // 已访问则跳过：防御异常脏数据造成的重复处理
            if !visited.insert(child.id) {
                continue;
            }
            // 理论必然命中（children 由本层 parent_ids 过滤而来）；用 get 而非索引是
            // 防御：将来若 children 查询加额外过滤，缺失时跳过而不是 panic
            let Some(parent_path) = path_map.get(&child.parent_id) else {
                continue;
            };
            let child_path = format!("{parent_path}{}/", child.id);
            // 后代只重算 dept_path（parent_id 不变）并盖操作人，其余列 NotSet
            let child_model = sys_dept::ActiveModel {
                id: Set(child.id),
                dept_path: Set(child_path.clone()),
                updated_by: Set(actor_id),
                ..Default::default()
            };
            child_model.update(txn).await?;
            next.push((child.id, child_path));
        }

        current = next;
    }
    Ok(Some(updated_model))
}

/// 事务内软删部门（关系/引用约束由 service 层负责）：只更新 `deleted_at` + `updated_by`。
///
/// 返回是否有行被更新（`false` = 目标不存在或已软删）；不产出业务错误文案，
/// 「部门不存在」的判定与报错由 service 层负责。
pub async fn soft_delete_dept_in_tx(
    txn: &DatabaseTransaction,
    dept_id: u64,
    actor_id: u64,
) -> anyhow::Result<bool> {
    let result = sys_dept::Entity::update_many()
        .filter(sys_dept::Column::Id.eq(dept_id))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .col_expr(
            sys_dept::Column::DeletedAt,
            Expr::value(Some(chrono::Local::now().naive_local())),
        )
        .col_expr(sys_dept::Column::UpdatedBy, Expr::value(actor_id))
        .exec(txn)
        .await?;
    Ok(result.rows_affected > 0)
}

/// 按 id 查询有效部门（排除软删；停用部门仍返回）。
pub async fn find_by_id(
    db: &impl ConnectionTrait,
    id: u64,
) -> anyhow::Result<Option<sys_dept::Model>> {
    let model = sys_dept::Entity::find()
        .filter(sys_dept::Column::Id.eq(id))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 按 id 查询有效部门并加排他锁（SQL 落到 `SELECT ... FOR UPDATE`）。
///
/// 供「读 → 判断 → 写」型校验使用：加锁读取的是**最新已提交版本**（绕过 RR 事务
/// 快照，普通读 `find_by_id` 看不到并发事务已提交的改动），同时与其他事务的加锁读 /
/// 写互斥，从而把校验变成临界区——并发下必有一方读到对方的结果并被规则拒绝。
///
/// **必须在事务内调用**：锁在事务结束时才释放，autocommit 下单条语句执行完即释放，
/// 等于没加锁。调用方需锁多行时按 id 升序逐个加锁（见 service 层 `lock_move_rows`），
/// 顺序不一致会交叉等待触发 `Deadlock found`。
pub async fn find_by_id_for_update(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<Option<sys_dept::Model>> {
    let model = sys_dept::Entity::find()
        .filter(sys_dept::Column::Id.eq(id))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(txn)
        .await?;
    Ok(model)
}

/// 批量按 id 查有效部门（排除软删；停用部门仍返回）。空入参短路，不产生空 `IN`。
///
/// 供跨域引用校验（如 user 挂载部门的存在性）与批量名称拼装使用。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> anyhow::Result<Vec<sys_dept::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let models = sys_dept::Entity::find()
        .filter(sys_dept::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(models)
}

/// 批量按 id 查有效部门并加排他锁（`SELECT ... FOR UPDATE`）。空入参短路。
///
/// 供「挂载引用」前的存在性校验使用：跨域写入（user 域写 `sys_user_dept`）必须先锁住
/// 被引用的部门行，与「删除部门」的占用检查在同一行上串行化——否则删除方检查完引用、
/// 挂载方随后插入，会留下指向已软删部门的悬挂引用。
///
/// 主键等值/`IN` 只取记录锁（无间隙锁），不阻塞 `sys_dept` 的自增插入。须在事务内调用。
pub async fn find_by_ids_for_update(
    txn: &DatabaseTransaction,
    ids: &[u64],
) -> anyhow::Result<Vec<sys_dept::Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let models = sys_dept::Entity::find()
        .filter(sys_dept::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .lock_exclusive()
        .all(txn)
        .await?;
    Ok(models)
}

/// 按父部门 + 部门名查有效部门（活数据查重用）。
pub async fn find_by_parent_and_name(
    db: &impl ConnectionTrait,
    parent_id: u64,
    dept_name: &str,
) -> anyhow::Result<Option<sys_dept::Model>> {
    let model = sys_dept::Entity::find()
        .filter(sys_dept::Column::ParentId.eq(parent_id))
        .filter(sys_dept::Column::DeptName.eq(dept_name))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 有效子部门列表并加排他锁（`SELECT ... FOR UPDATE`），供删除前的「无活子部门」判定。
///
/// 普通读在 RR 下走事务快照，会漏掉并发事务已提交的新建子部门，从而删出游离子树。
/// 查询走 `uk_sys_dept_parent_name(parent_id, ...)` 的**前缀**范围（非整键唯一），
/// 因此会取到 next-key / 间隙锁，与并发「在该父下插入子部门」的插入意向锁互斥
///（依赖 RR 隔离级别：READ COMMITTED 下没有间隙锁）。
pub async fn find_children_for_update(
    txn: &DatabaseTransaction,
    parent_id: u64,
) -> anyhow::Result<Vec<sys_dept::Model>> {
    let models = sys_dept::Entity::find()
        .filter(sys_dept::Column::ParentId.eq(parent_id))
        .filter(sys_dept::Column::DeletedAt.is_null())
        .order_by_asc(sys_dept::Column::Sort)
        .order_by_asc(sys_dept::Column::Id)
        .lock_exclusive()
        .all(txn)
        .await?;
    Ok(models)
}

/// 全量有效部门（排除软删；**含停用**，树组装需展示停用节点），按 id 升序。
pub async fn find_all_active(db: &impl ConnectionTrait) -> anyhow::Result<Vec<sys_dept::Model>> {
    let models = sys_dept::Entity::find()
        .filter(sys_dept::Column::DeletedAt.is_null())
        .order_by_asc(sys_dept::Column::Id)
        .all(db)
        .await?;
    Ok(models)
}

/// 取挂载到该部门的用户引用行并加排他锁（`sys_user_dept` 硬删表，无需软删过滤），
/// 供删除部门前的「无用户引用」判定。
///
/// 返回行而非计数：`.count()` 会被 sea-orm 包成 `SELECT COUNT(*) FROM (SELECT ...)`
/// 派生表（`Paginator::num_items`），`FOR UPDATE` 落在派生表内层、MySQL 不可靠，故取行后
/// 由调用方判空。走 `idx_sys_user_dept_dept_id`（非唯一索引）⇒ 有间隙锁，与并发挂载
///（user 域插入 `sys_user_dept`）的插入意向锁互斥；依赖 RR 隔离级别（RC 下没有间隙锁）。
pub async fn find_user_refs_by_dept_id_for_update(
    txn: &DatabaseTransaction,
    dept_id: u64,
) -> anyhow::Result<Vec<sys_user_dept::Model>> {
    let rows = sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::DeptId.eq(dept_id))
        .lock_exclusive()
        .all(txn)
        .await?;
    Ok(rows)
}

/// 按部门 id 集合查负责人（`is_leader = 1`）及其显示名，返回
/// `(dept_id, user_id, username)`。
///
/// 用户不过滤软删：负责人展示面向历史引用，与 `utils::user_ref` 的名称解析口径一致。
/// 空入参短路，不产生空 `IN`。
pub async fn find_leaders_by_dept_ids(
    db: &impl ConnectionTrait,
    dept_ids: &[u64],
) -> anyhow::Result<Vec<(u64, u64, String)>> {
    if dept_ids.is_empty() {
        return Ok(Vec::new());
    }
    let links = sys_user_dept::Entity::find()
        .filter(sys_user_dept::Column::DeptId.is_in(dept_ids.iter().copied()))
        .filter(sys_user_dept::Column::IsLeader.eq(1))
        .all(db)
        .await?;
    if links.is_empty() {
        return Ok(Vec::new());
    }

    let user_ids: Vec<u64> = links.iter().map(|link| link.user_id).collect();
    let users = sys_user::Entity::find()
        .filter(sys_user::Column::Id.is_in(user_ids))
        .all(db)
        .await?;
    let name_map: HashMap<u64, String> = users
        .into_iter()
        .map(|user| (user.id, user.username))
        .collect();

    Ok(links
        .into_iter()
        .map(|link| {
            let name = name_map.get(&link.user_id).cloned().unwrap_or_default();
            (link.dept_id, link.user_id, name)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, TransactionTrait};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        test_db().await.begin().await.unwrap()
    }

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn seed_root(txn: &DatabaseTransaction, dept_name: &str) -> sys_dept::Model {
        create_dept_in_tx(
            txn,
            sys_dept::ActiveModel {
                dept_name: Set(dept_name.to_string()),
                parent_id: Set(0),
                ..Default::default()
            },
            "/0/",
            ACTOR_ID,
        )
        .await
        .unwrap()
    }

    async fn seed_child(
        txn: &DatabaseTransaction,
        parent: &sys_dept::Model,
        dept_name: &str,
    ) -> sys_dept::Model {
        create_dept_in_tx(
            txn,
            sys_dept::ActiveModel {
                dept_name: Set(dept_name.to_string()),
                parent_id: Set(parent.id),
                ..Default::default()
            },
            &parent.dept_path,
            ACTOR_ID,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    // 根部门 path 规范：占位 /0/ + 自身 id。
    async fn create_dept_root_writes_canonical_path() {
        let txn = test_txn().await;
        let root = seed_root(&txn, &unique("root_path")).await;

        assert_eq!(root.dept_path, format!("/0/{}/", root.id));
        assert_eq!(root.parent_id, 0);
    }

    #[tokio::test]
    // 子部门 path 规范：父 path + 自身 id；两层链可见。
    async fn create_dept_child_appends_id_to_parent_path() {
        let txn = test_txn().await;
        let root = seed_root(&txn, &unique("child_path_root")).await;
        let child = seed_child(&txn, &root, &unique("child_path_dept")).await;
        let grand = seed_child(&txn, &child, &unique("child_path_grand")).await;

        assert_eq!(child.dept_path, format!("{}{}/", root.dept_path, child.id));
        assert_eq!(grand.dept_path, format!("{}{}/", child.dept_path, grand.id));
    }

    #[tokio::test]
    async fn create_dept_stamps_actor_as_creator_and_updater() {
        let txn = test_txn().await;
        let created = seed_root(&txn, &unique("root_stamp")).await;

        assert_eq!(created.created_by, ACTOR_ID);
        assert_eq!(created.updated_by, ACTOR_ID);
    }

    #[tokio::test]
    // 同父同层重名被拒（唯一键 parent_id + dept_name + 软删占位）。
    async fn create_dept_rejects_duplicate_name_under_same_parent() {
        let txn = test_txn().await;
        let root = seed_root(&txn, &unique("dup_root")).await;
        let name = unique("dup_dept");
        let _first = seed_child(&txn, &root, &name).await;

        let found = find_by_parent_and_name(&txn, root.id, &name).await.unwrap();
        assert_eq!(found.as_ref().map(|d| d.id), Some(_first.id));

        let dup = create_dept_in_tx(
            &txn,
            sys_dept::ActiveModel {
                dept_name: Set(name),
                parent_id: Set(root.id),
                ..Default::default()
            },
            &root.dept_path,
            ACTOR_ID,
        )
        .await;

        assert!(dup.is_err(), "同父同层部门名重复应被唯一键拒绝");
    }

    #[tokio::test]
    // 软删占位（沿用 sys_config.config_key 同款语义）：软删后不可见，且同父同名
    // 不可重建——唯一键 (parent_id, dept_name) 被软删行继续占用，防历史引用歧义。
    async fn soft_delete_dept_hides_row_and_reserves_same_parent_name() {
        let txn = test_txn().await;
        let root = seed_root(&txn, &unique("reserve_root")).await;
        let name = unique("reserve_dept");
        let victim = seed_child(&txn, &root, &name).await;

        let removed = soft_delete_dept_in_tx(&txn, victim.id, ACTOR_ID)
            .await
            .unwrap();
        assert!(removed);
        assert!(
            find_by_id(&txn, victim.id).await.unwrap().is_none(),
            "软删部门不应再被 find_by_id 查到"
        );
        assert!(
            find_by_parent_and_name(&txn, root.id, &name)
                .await
                .unwrap()
                .is_none(),
            "查重辅助只查活数据，软删行应被排除"
        );

        // 同名重建应失败：软删行仍占用同父唯一键
        let recreate = create_dept_in_tx(
            &txn,
            sys_dept::ActiveModel {
                dept_name: Set(name),
                parent_id: Set(root.id),
                ..Default::default()
            },
            &root.dept_path,
            ACTOR_ID,
        )
        .await;

        assert!(recreate.is_err(), "同父同名应被软删行占位拒绝");
        let all = find_all_active(&txn).await.unwrap();
        assert!(
            all.iter().all(|d| d.id != victim.id),
            "全量列表应排除软删部门"
        );
    }

    #[tokio::test]
    // 普通字段更新生效，且只刷 updated_by、保留 created_by。
    async fn update_dept_refreshes_updated_by_and_keeps_created_by() {
        let txn = test_txn().await;
        let root = seed_root(&txn, &unique("update_root")).await;
        const UPDATER_ID: u64 = 2;

        let mut model: sys_dept::ActiveModel = root.clone().into();
        model.allow_peer_read = Set(1);
        model.remark = Set("同级互看开放".to_string());
        let updated = update_dept_in_tx(&txn, model, UPDATER_ID).await.unwrap();

        assert_eq!(updated.allow_peer_read, 1);
        assert_eq!(updated.remark, "同级互看开放");
        assert_eq!(updated.created_by, ACTOR_ID, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, UPDATER_ID);
    }

    #[tokio::test]
    // 停用（status=0）应被 find_by_id 查到（get/update 场景停用不等于不存在），
    // 软删必须查不到。
    async fn find_by_id_excludes_soft_deleted_but_keeps_disabled() {
        let txn = test_txn().await;
        let disabled = seed_root(&txn, &unique("find_disabled")).await;
        let deleted = seed_root(&txn, &unique("find_deleted")).await;

        // 停用 + 软删
        let mut model: sys_dept::ActiveModel = disabled.clone().into();
        model.status = Set(0);
        update_dept_in_tx(&txn, model, ACTOR_ID).await.unwrap();
        soft_delete_dept_in_tx(&txn, deleted.id, ACTOR_ID)
            .await
            .unwrap();

        let found_disabled = find_by_id(&txn, disabled.id).await.unwrap();
        let found_deleted = find_by_id(&txn, deleted.id).await.unwrap();
        assert_eq!(
            found_disabled.as_ref().map(|d| d.id),
            Some(disabled.id),
            "停用部门仍应被 find_by_id 查到"
        );
        assert!(found_deleted.is_none(), "软删部门不应被 find_by_id 查到");
    }

    #[tokio::test]
    // 批量查询：只返回传入 id 中有效（未软删）的部门；空入参短路返回空数组。
    async fn find_by_ids_returns_only_active_matching_depts() {
        let txn = test_txn().await;
        let live_a = seed_root(&txn, &unique("by_ids_a")).await;
        let live_b = seed_root(&txn, &unique("by_ids_b")).await;
        let gone = seed_root(&txn, &unique("by_ids_gone")).await;
        soft_delete_dept_in_tx(&txn, gone.id, ACTOR_ID)
            .await
            .unwrap();

        let found = find_by_ids(&txn, &[live_a.id, live_b.id, gone.id])
            .await
            .unwrap();
        let ids: Vec<u64> = found.iter().map(|d| d.id).collect();
        assert_eq!(found.len(), 2, "软删部门不应出现在批量结果");
        assert!(ids.contains(&live_a.id) && ids.contains(&live_b.id));

        assert!(
            find_by_ids(&txn, &[]).await.unwrap().is_empty(),
            "空入参应短路返回空数组"
        );
    }

    #[tokio::test]
    // find_children_for_update 只返回活子部门，软删子部门剔除；排序按 sort、id 升序。
    async fn find_children_for_update_returns_only_active_children() {
        let txn = test_txn().await;
        let root = seed_root(&txn, &unique("children_root")).await;
        let live = seed_child(&txn, &root, &unique("children_live")).await;
        let gone = seed_child(&txn, &root, &unique("children_gone")).await;
        soft_delete_dept_in_tx(&txn, gone.id, ACTOR_ID)
            .await
            .unwrap();

        let children = find_children_for_update(&txn, root.id).await.unwrap();
        let ids = children.iter().map(|d| d.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![live.id]);
    }

    #[tokio::test]
    // 防环外的正确移动：换父后自身与整棵子树 path 按新父链重算，原父链不受影响。
    async fn move_subtree_recomputes_root_and_descendant_paths() {
        let txn = test_txn().await;
        let root_a = seed_root(&txn, &unique("move_a")).await;
        let root_b = seed_root(&txn, &unique("move_b")).await;
        let dept = seed_child(&txn, &root_a, &unique("move_dept")).await;
        let grand = seed_child(&txn, &dept, &unique("move_grand")).await;

        // 把 dept 从 root_a 移入 root_b
        let moved = move_subtree_in_tx(&txn, dept.id, root_b.id, &root_b.dept_path, ACTOR_ID)
            .await
            .unwrap()
            .expect("部门应存在");

        let expect_dept_path = format!("{}{}/", root_b.dept_path, dept.id);
        assert_eq!(moved.parent_id, root_b.id);
        assert_eq!(moved.dept_path, expect_dept_path);

        let reloaded_grand = find_by_id(&txn, grand.id).await.unwrap().unwrap();
        assert_eq!(
            reloaded_grand.dept_path,
            format!("{}{}/", expect_dept_path, grand.id),
            "孙节点 path 应随移动整体重算"
        );

        // 原父链路径与结构不受影响
        let reloaded_a = find_by_id(&txn, root_a.id).await.unwrap().unwrap();
        assert_eq!(reloaded_a.dept_path, format!("/0/{}/", root_a.id));
        let a_children = find_children_for_update(&txn, root_a.id).await.unwrap();
        assert!(
            a_children.iter().all(|c| c.id != dept.id),
            "移动后原父下不应再有其子节点"
        );
    }

    #[tokio::test]
    // 审计：被移动节点与全部后代的 updated_by 统一刷成本次操作人。
    async fn move_subtree_stamps_actor_on_root_and_descendants() {
        let txn = test_txn().await;
        const UPDATER_ID: u64 = 2;
        let root_a = seed_root(&txn, &unique("stamp_a")).await;
        let root_b = seed_root(&txn, &unique("stamp_b")).await;
        let dept = seed_child(&txn, &root_a, &unique("stamp_dept")).await;
        let grand = seed_child(&txn, &dept, &unique("stamp_grand")).await;
        assert_eq!(dept.updated_by, ACTOR_ID, "前置：创建时盖的是创建人");

        let moved = move_subtree_in_tx(&txn, dept.id, root_b.id, &root_b.dept_path, UPDATER_ID)
            .await
            .unwrap()
            .expect("部门应存在");
        let reloaded_grand = find_by_id(&txn, grand.id).await.unwrap().unwrap();

        assert_eq!(moved.updated_by, UPDATER_ID, "被移动节点应盖操作人");
        assert_eq!(reloaded_grand.updated_by, UPDATER_ID, "后代应一并盖操作人");
    }

    #[tokio::test]
    // 父未变更 = no-op：不改 path、不刷新 updated_by。
    async fn move_subtree_to_same_parent_is_noop() {
        let txn = test_txn().await;
        const UPDATER_ID: u64 = 2;
        let root = seed_root(&txn, &unique("noop_root")).await;
        let dept = seed_child(&txn, &root, &unique("noop_dept")).await;

        // 目标父 == 当前父
        let result = move_subtree_in_tx(&txn, dept.id, root.id, &root.dept_path, UPDATER_ID)
            .await
            .unwrap()
            .expect("部门应存在");

        assert_eq!(result.dept_path, dept.dept_path, "no-op 不应改 path");
        assert_eq!(result.updated_by, ACTOR_ID, "no-op 不应刷新操作人");
    }

    #[tokio::test]
    // 防御性：库内存在环状脏数据时，下推仍应正常终止（不挂死）并产出正确 path。
    // 构造 a→c→b→a 三节点环（绕过 service 防环直接改库）。
    async fn move_subtree_terminates_on_cyclic_dirty_data() {
        let txn = test_txn().await;
        let target_root = seed_root(&txn, &unique("cyc_target")).await;
        let a = seed_root(&txn, &unique("cyc_a")).await;
        let b = seed_child(&txn, &a, &unique("cyc_b")).await;
        let c = seed_child(&txn, &b, &unique("cyc_c")).await;

        // 制造环：把 a 的父指向孙节点 c（a 的下级），形成 a→c→b→a
        let mut dirty: sys_dept::ActiveModel = a.clone().into();
        dirty.parent_id = Set(c.id);
        dirty.update(&txn).await.unwrap();

        // 移动 a：遍历必须终止，且被移动节点 path 按新父链重算
        let moved =
            move_subtree_in_tx(&txn, a.id, target_root.id, &target_root.dept_path, ACTOR_ID)
                .await
                .unwrap()
                .expect("部门应存在");

        assert_eq!(moved.parent_id, target_root.id);
        assert_eq!(
            moved.dept_path,
            format!("{}{}/", target_root.dept_path, a.id)
        );
    }

    #[tokio::test]
    // 宽树：同层多个子节点全部重算（验证 path_map 批量映射）。
    async fn move_subtree_handles_multiple_children_per_level() {
        let txn = test_txn().await;
        let root_a = seed_root(&txn, &unique("wide_a")).await;
        let root_b = seed_root(&txn, &unique("wide_b")).await;
        let dept = seed_child(&txn, &root_a, &unique("wide_dept")).await;
        let c1 = seed_child(&txn, &dept, &unique("wide_c1")).await;
        let c2 = seed_child(&txn, &dept, &unique("wide_c2")).await;
        let c3 = seed_child(&txn, &dept, &unique("wide_c3")).await;

        let moved = move_subtree_in_tx(&txn, dept.id, root_b.id, &root_b.dept_path, ACTOR_ID)
            .await
            .unwrap()
            .expect("部门应存在");

        for child in [c1, c2, c3] {
            let reloaded = find_by_id(&txn, child.id).await.unwrap().unwrap();
            assert_eq!(
                reloaded.dept_path,
                format!("{}{}/", moved.dept_path, child.id),
                "同层每个子节点都应重算 path"
            );
        }
    }

    #[tokio::test]
    async fn find_all_active_excludes_soft_deleted() {
        let txn = test_txn().await;
        let live = seed_root(&txn, &unique("all_live")).await;
        let deleted = seed_root(&txn, &unique("all_deleted")).await;
        soft_delete_dept_in_tx(&txn, deleted.id, ACTOR_ID)
            .await
            .unwrap();

        let all = find_all_active(&txn).await.unwrap();
        let ids = all.iter().map(|d| d.id).collect::<Vec<_>>();
        assert!(ids.contains(&live.id));
        assert!(!ids.contains(&deleted.id), "全量列表应排除软删部门");
    }
}
