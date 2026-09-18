//! 菜单域数据访问。

use crate::entity::sys_role_menu;
use crate::entity::{sys_menu, sys_menu::Model};
use crate::modules::system::menu::dto::MenuFilter;
use crate::utils::PageData;
use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, DatabaseTransaction, QueryOrder, QuerySelect};
use std::collections::HashSet;

/// 查询全部启用菜单（超管全量菜单树用；按角色过滤见 `find_menus_by_role_ids`）。
pub async fn find_all_menus(db: &impl ConnectionTrait) -> anyhow::Result<Vec<Model>> {
    Ok(sys_menu::Entity::find()
        .filter(sys_menu::Column::Status.eq(1))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .order_by_asc(sys_menu::Column::Sort)
        .all(db)
        .await?)
}

/// 分页 + 动态过滤查询（keyword 匹配 name，status/menu_type 精确，
/// created_by/updated_by/时间范围审计过滤），排除软删。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &MenuFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<PageData<Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = &filter.keyword {
        cond = cond.add(sys_menu::Column::Name.like(format!("%{}%", keyword)));
    }

    if let Some(status) = filter.status {
        cond = cond.add(sys_menu::Column::Status.eq(status));
    }

    if let Some(menu_type) = filter.menu_type {
        cond = cond.add(sys_menu::Column::MenuType.eq(menu_type));
    }

    // 审计过滤：人字段精确（种子/系统写入为 0），时间为含边界范围
    if let Some(v) = filter.created_by {
        cond = cond.add(sys_menu::Column::CreatedBy.eq(v));
    }
    if let Some(v) = filter.updated_by {
        cond = cond.add(sys_menu::Column::UpdatedBy.eq(v));
    }
    if let Some(v) = filter.created_at_begin {
        cond = cond.add(sys_menu::Column::CreatedAt.gte(v));
    }
    if let Some(v) = filter.created_at_end {
        cond = cond.add(sys_menu::Column::CreatedAt.lte(v));
    }
    if let Some(v) = filter.updated_at_begin {
        cond = cond.add(sys_menu::Column::UpdatedAt.gte(v));
    }
    if let Some(v) = filter.updated_at_end {
        cond = cond.add(sys_menu::Column::UpdatedAt.lte(v));
    }
    let select = sys_menu::Entity::find()
        .filter(cond)
        .filter(sys_menu::Column::DeletedAt.is_null())
        // 分页必须有确定性排序：无 ORDER BY 时行序由执行计划决定，
        // LIMIT/OFFSET 会出现跨页重复或永久漏项。统一按主键降序（新数据在前）。
        .order_by_desc(sys_menu::Column::Id);

    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 查询单个有效菜单（排除软删除）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let menu = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.eq(id))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(menu)
}

/// 查询单个有效菜单（排除软删除）并加排他锁（`SELECT ... FOR UPDATE`）。
///
/// 供「读 → 判断 → 写」型校验使用：加锁读取最新已提交版本（绕过 RR 事务快照），
/// 并与其他事务的加锁读/写在同一行上互斥，使校验成为临界区。须在事务内调用。
pub async fn find_by_id_for_update(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<Option<Model>> {
    let menu = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.eq(id))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(txn)
        .await?;
    Ok(menu)
}

/// 批量按 id 查有效菜单（排除软删）并加排他锁（`SELECT ... FOR UPDATE`）。空入参短路。
///
/// 供「角色绑定菜单」前的存在性校验使用：跨域写入（role 域写 `sys_role_menu`）必须先锁住
/// 被引用的菜单行，与「删除菜单」的级联清理在同一行上串行化——否则删除方清完关联、
/// 绑定方随后插入，会留下指向已软删菜单的悬挂绑定。须在事务内调用；主键等值/`IN`
/// 只取记录锁（无间隙锁），不阻塞自增插入。
pub async fn find_by_ids_for_update(
    txn: &DatabaseTransaction,
    ids: &[u64],
) -> anyhow::Result<Vec<Model>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let menus = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .lock_exclusive()
        .all(txn)
        .await?;
    Ok(menus)
}

/// 创建菜单（ActiveModel 入参，默认值由 service 层负责）。
/// 创建菜单。`actor_id` 为操作人，审计字段由 repo 统一盖章。
pub async fn create_menu(
    db: &impl ConnectionTrait,
    model: sys_menu::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Model> {
    // 创建场景：创建人与更新人同源
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    let model = model.insert(db).await?;
    Ok(model)
}

/// 更新菜单（主键必须已设置，全量覆盖语义）。
/// 审计字段由 repo 统一盖章：只刷新更新人，`created_by` 保持 NotSet 不被覆盖。
pub async fn update_menu(
    db: &impl ConnectionTrait,
    model: sys_menu::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Model> {
    let mut model = model;
    model.updated_by = Set(actor_id);
    let model = model.update(db).await?;
    Ok(model)
}

/// 收集 `root_id` 的全部子孙菜单 id 并加排他锁（**不含** root 自身，**不过滤软删**）。
///
/// `visited` 防御：库内若存在环状脏数据（`A→B→A`），遍历必须终止而不是无限扩展。
/// 返回 id 集合而非业务判定——「新父是否为自身子孙」的策略由 service 层决定。
///
/// 加锁读（`SELECT ... FOR UPDATE`）是防环与级联删除的正确性前提：
/// - 普通读在 RR 下走事务快照，两个并发互移请求各自看不到对方已提交的换父，会双双通过
///   防环校验而成环；加锁读既读最新已提交版本，又让两端在同样的行上串行化；
/// - 级联软删若用快照读，会漏掉并发新建的子孙，留下「父已删、子还在」的孤立菜单。
///
/// 逐层**自顶向下**加锁（先父行、后其子节点区间），与删除、与并发「在子节点下建子」的
/// 插入意向锁方向一致，不构成循环等待。走 `idx_sys_menu_parent_id`（非唯一索引）
/// ⇒ 有间隙锁，依赖 RR 隔离级别。须在事务内调用。
pub async fn find_descendant_ids_for_update(
    txn: &DatabaseTransaction,
    root_id: u64,
) -> anyhow::Result<Vec<u64>> {
    let mut descendants = Vec::new();
    // visited 先放入 root：root 不会被当作自身的子孙重复收集
    let mut visited: HashSet<u64> = HashSet::from([root_id]);
    let mut frontier = vec![root_id];
    while !frontier.is_empty() {
        let children = sys_menu::Entity::find()
            .filter(sys_menu::Column::ParentId.is_in(frontier))
            .column(sys_menu::Column::Id)
            .lock_exclusive()
            .all(txn)
            .await?;
        let mut next = Vec::with_capacity(children.len());
        for child in children {
            // 已访问则跳过：环状脏数据下不再入队，保证遍历必然终止
            if visited.insert(child.id) {
                descendants.push(child.id);
                next.push(child.id);
            }
        }
        frontier = next;
    }
    Ok(descendants)
}

/// 软删除菜单（不 begin/commit，事务边界由调用方负责）：收集目标及其全部子孙菜单，
/// 清空这些菜单的角色关联并批量软删主表（子孙收集走 `find_descendant_ids_for_update`，
/// 带 `visited` 防环，且加锁读避免漏掉并发新建的子孙）。
///
/// 子级收集**不过滤软删状态**：即使某个中间节点已软删，其下仍正常的子孙也要级联处理，
/// 避免父已删、子孤立的脏数据。
pub(crate) async fn soft_delete_menu_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
) -> anyhow::Result<bool> {
    // 目标菜单必须存在（排除软删）并加锁，否则视为无可删除
    let Some(menu) = find_by_id_for_update(txn, id).await? else {
        return Ok(false);
    };

    // 目标 + 全部子孙 id（子孙收集复用防环原语：脏数据成环时遍历必然终止）
    let mut ids = vec![menu.id];
    ids.extend(find_descendant_ids_for_update(txn, menu.id).await?);

    let now = chrono::Local::now().naive_local();
    // 关系表硬删除：清掉这些菜单的角色绑定
    sys_role_menu::Entity::delete_many()
        .filter(sys_role_menu::Column::MenuId.is_in(ids.clone()))
        .exec(txn)
        .await?;
    // 主表批量软删（含目标与全部子孙）
    sys_menu::Entity::update_many()
        .col_expr(
            sys_menu::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(Some(now)),
        )
        .filter(sys_menu::Column::Id.is_in(ids))
        .exec(txn)
        .await?;
    Ok(true)
}

/// 查重辅助：菜单 name 唯一（含软删占位）。
pub async fn find_by_name_include_deleted(
    db: &impl ConnectionTrait,
    name: &str,
) -> anyhow::Result<Option<Model>> {
    let menu = sys_menu::Entity::find()
        .filter(sys_menu::Column::Name.eq(name))
        .one(db)
        .await?;
    Ok(menu)
}

/// 按角色集合查可见菜单：sys_role_menu → 启用且未软删的菜单。
///
/// 角色解析（有效角色 = 启用且未删）由 service 层负责，repo 不跨域查用户域；
/// `role_ids` 为空直接返回空，避免生成空 IN 的无效 SQL。
pub async fn find_menus_by_role_ids(
    db: &impl ConnectionTrait,
    role_ids: &[u64],
) -> anyhow::Result<Vec<Model>> {
    if role_ids.is_empty() {
        return Ok(vec![]);
    }

    let menu_ids = sys_role_menu::Entity::find()
        .filter(sys_role_menu::Column::RoleId.is_in(role_ids.iter().copied()))
        .column(sys_role_menu::Column::MenuId)
        .all(db)
        .await?;
    let menu_ids = menu_ids.into_iter().map(|m| m.menu_id).collect::<Vec<_>>();
    if menu_ids.is_empty() {
        return Ok(vec![]);
    }

    let menus = sys_menu::Entity::find()
        .filter(sys_menu::Column::Id.is_in(menu_ids))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .filter(sys_menu::Column::Status.eq(1))
        .all(db)
        .await?;
    Ok(menus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_menu;
    use crate::modules::system::menu::dto::MenuFilter;
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    fn unique(prefix: &str) -> String {
        format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )
    }

    async fn test_db() -> DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    async fn seed_menu(
        db: &impl ConnectionTrait,
        name: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(0),
            path: Set(format!("/{}", unique("seed_path"))),
            name: Set(name.to_string()),
            component: Set(format!("#/views/{}.vue", unique("seed_comp"))),
            title: Set(unique("seed_title")),
            icon: Set(String::new()),
            sort: Set(0),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 造带指定父节点的子菜单：用于构造级联 / 环状脏数据。
    async fn seed_menu_with_parent(
        db: &impl ConnectionTrait,
        name: &str,
        parent_id: u64,
    ) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(parent_id),
            path: Set(format!("/{}", unique("seed_path"))),
            name: Set(name.to_string()),
            component: Set(format!("#/views/{}.vue", unique("seed_comp"))),
            title: Set(unique("seed_title")),
            icon: Set(String::new()),
            sort: Set(0),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 创建菜单：ActiveModel 显式字段按值落库。
    #[tokio::test]
    async fn create_menu_persists_fields_and_defaults() {
        let db = test_txn().await;
        let name = unique("create_menu");
        let model = sys_menu::ActiveModel {
            parent_id: Set(0),
            path: Set(format!("/{name}")),
            name: Set(name.clone()),
            component: Set(format!("#/views/{name}.vue")),
            title: Set(format!("T{name}")),
            icon: Set("mdi:test".to_string()),
            sort: Set(5),
            keep_alive: Set(0),
            hidden: Set(1),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        };

        let created = create_menu(&db, model, ACTOR_ID).await.unwrap();

        assert_eq!(created.name, name);
        assert_eq!(created.parent_id, 0);
        assert_eq!(created.icon, "mdi:test");
        assert_eq!(created.sort, 5);
        assert_eq!(created.hidden, 1);
        assert_eq!(created.keep_alive, 0);
        assert_eq!(created.menu_type, 1);
        assert_eq!(created.status, 1);
        assert_eq!(created.permission, "");
    }

    /// find_by_id 应排除软删除菜单。
    #[tokio::test]
    async fn find_by_id_excludes_soft_deleted_menu() {
        let db = test_txn().await;
        let live = seed_menu(&db, &unique("find_live"), 1, None).await;
        let deleted = seed_menu(
            &db,
            &unique("find_deleted"),
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let found_live = find_by_id(&db, live.id).await.unwrap();
        let found_deleted = find_by_id(&db, deleted.id).await.unwrap();

        assert_eq!(found_live.as_ref().map(|m| m.id), Some(live.id));
        assert!(found_deleted.is_none(), "软删除菜单不应被 find_by_id 查到");
    }

    /// 分页：keyword 命中 name，status/menu_type 精确过滤，排除软删。
    #[tokio::test]
    async fn find_page_filters_by_keyword_status_and_menu_type_excludes_deleted() {
        let db = test_txn().await;
        let keyword = unique("page_keyword");
        let _live = seed_menu(&db, &format!("{keyword}_live"), 1, None).await;
        let _disabled = seed_menu(&db, &format!("{keyword}_disabled"), 0, None).await;
        let _deleted = seed_menu(
            &db,
            &format!("{keyword}_deleted"),
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let data_all = find_page(
            &db,
            &MenuFilter {
                keyword: Some(keyword.clone()),
                status: None,
                menu_type: None,
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let data_enabled = find_page(
            &db,
            &MenuFilter {
                keyword: Some(keyword.clone()),
                status: Some(1),
                menu_type: None,
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        let data_buttons = find_page(
            &db,
            &MenuFilter {
                keyword: Some(keyword.clone()),
                status: None,
                menu_type: Some(3),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(data_all.total, 2, "软删除菜单不应进入分页");
        assert_eq!(data_enabled.total, 1, "status 过滤应只保留启用菜单");
        assert_eq!(data_buttons.total, 0, "menu_type 过滤应匹配按钮类型");
    }

    /// 软删除：deleted_at 被置为当前时间，且随后 find_by_id 查不到。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn soft_delete_menu_sets_deleted_at() {
        let txn = test_txn().await;
        let menu = seed_menu(&txn, &unique("soft_delete"), 1, None).await;

        let deleted = soft_delete_menu_in_tx(&txn, menu.id).await.unwrap();
        let after = find_by_id(&txn, menu.id).await.unwrap();

        assert!(deleted);
        assert!(after.is_none(), "软删后 find_by_id 不应查到菜单");
    }

    /// 级联软删：删除父菜单时，所有子孙菜单一起软删（BFS 递归）。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn soft_delete_menu_cascades_to_all_descendants() {
        let txn = test_txn().await;
        let parent = seed_menu(&txn, &unique("cascade_parent"), 1, None).await;
        let child = sys_menu::ActiveModel {
            parent_id: Set(parent.id),
            path: Set(format!("/{}", unique("child_path"))),
            name: Set(unique("cascade_child")),
            component: Set(format!("#/views/{}.vue", unique("child_comp"))),
            title: Set(unique("child_title")),
            icon: Set(String::new()),
            sort: Set(1),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .unwrap();
        let grandchild = sys_menu::ActiveModel {
            parent_id: Set(child.id),
            path: Set(format!("/{}", unique("grand_path"))),
            name: Set(unique("cascade_grandchild")),
            component: Set(format!("#/views/{}.vue", unique("grand_comp"))),
            title: Set(unique("grand_title")),
            icon: Set(String::new()),
            sort: Set(2),
            keep_alive: Set(0),
            hidden: Set(0),
            menu_type: Set(1),
            permission: Set(String::new()),
            status: Set(1),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .unwrap();

        let deleted = soft_delete_menu_in_tx(&txn, parent.id).await.unwrap();
        let parent_after = find_by_id(&txn, parent.id).await.unwrap();
        let child_after = find_by_id(&txn, child.id).await.unwrap();
        let grandchild_after = find_by_id(&txn, grandchild.id).await.unwrap();

        assert!(deleted);
        assert!(parent_after.is_none(), "父菜单应被软删");
        assert!(child_after.is_none(), "子菜单应被级联软删");
        assert!(grandchild_after.is_none(), "孙菜单应被级联软删");
    }

    /// 更新菜单：全量覆盖主表字段（与角色域全量更新语义一致）。
    #[tokio::test]
    async fn update_menu_overwrites_fields() {
        let db = test_txn().await;
        let menu = seed_menu(&db, &unique("update_before"), 1, None).await;
        let new_name = unique("update_after");

        let mut model: sys_menu::ActiveModel = menu.clone().into();
        model.path = Set(format!("/{new_name}"));
        model.name = Set(new_name.clone());
        model.component = Set(format!("#/views/{new_name}.vue"));
        model.title = Set(format!("T{new_name}"));
        model.icon = Set("mdi:updated".to_string());
        model.sort = Set(9);
        model.keep_alive = Set(1);
        model.hidden = Set(1);
        model.status = Set(1);

        let updated = update_menu(&db, model, ACTOR_ID).await.unwrap();

        assert_eq!(updated.name, new_name);
        assert_eq!(updated.title, format!("T{new_name}"));
        assert_eq!(updated.icon, "mdi:updated");
        assert_eq!(updated.sort, 9);
        assert_eq!(updated.keep_alive, 1);
    }

    /// 查重辅助：包含软删除记录（唯一索引占位检查用）。
    #[tokio::test]
    async fn find_by_name_include_deleted_finds_soft_deleted_menu() {
        let db = test_txn().await;
        let name = unique("dup_name");
        let deleted = seed_menu(&db, &name, 1, Some(chrono::Local::now().naive_local())).await;

        let found = find_by_name_include_deleted(&db, &name).await.unwrap();

        assert_eq!(found.as_ref().map(|m| m.id), Some(deleted.id));
    }

    /// 造一个操作人用户（直接 insert，不走 repo；其审计字段为 0（种子/系统写入口径）属预期）。
    async fn seed_actor(db: &impl ConnectionTrait) -> u64 {
        crate::entity::sys_user::ActiveModel {
            username: Set(unique("audit_actor")),
            password: Set("x".to_string()),
            nickname: Set("审计操作人".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn delete_actors(db: &impl ConnectionTrait, ids: &[u64]) {
        crate::entity::sys_user::Entity::delete_many()
            .filter(crate::entity::sys_user::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_menu_stamps_actor_as_creator_and_updater() {
        let db = test_txn().await;
        let actor_id = seed_actor(&db).await;

        let created = create_menu(
            &db,
            sys_menu::ActiveModel {
                parent_id: Set(0),
                path: Set(format!("/{}", unique("audit_path"))),
                name: Set(unique("AuditMenu")),
                component: Set(format!("#/views/{}.vue", unique("audit_comp"))),
                title: Set(unique("audit_title")),
                menu_type: Set(1),
                status: Set(1),
                ..Default::default()
            },
            actor_id,
        )
        .await
        .unwrap();

        assert_eq!(created.created_by, actor_id);
        assert_eq!(created.updated_by, actor_id);

        delete_actors(&db, &[actor_id]).await;
    }

    #[tokio::test]
    async fn update_menu_refreshes_updated_by_and_keeps_created_by() {
        let db = test_txn().await;
        let creator_id = seed_actor(&db).await;
        let updater_id = seed_actor(&db).await;

        let created = create_menu(
            &db,
            sys_menu::ActiveModel {
                parent_id: Set(0),
                path: Set(format!("/{}", unique("audit_path"))),
                name: Set(unique("AuditMenu")),
                component: Set(format!("#/views/{}.vue", unique("audit_comp"))),
                title: Set(unique("audit_title")),
                menu_type: Set(1),
                status: Set(1),
                ..Default::default()
            },
            creator_id,
        )
        .await
        .unwrap();

        let updated = update_menu(
            &db,
            sys_menu::ActiveModel {
                id: Set(created.id),
                title: Set(unique("audit_title_renamed")),
                ..Default::default()
            },
            updater_id,
        )
        .await
        .unwrap();

        assert_eq!(updated.created_by, creator_id, "创建人不应被更新覆盖");
        assert_eq!(updated.updated_by, updater_id);

        delete_actors(&db, &[creator_id, updater_id]).await;
    }

    /// 审计字段过滤：created_by/updated_by 精确 + created_at/updated_at 含边界范围。
    #[tokio::test]
    async fn find_page_filters_by_audit_columns_and_time_range() {
        let db = test_txn().await;
        let kw = unique("audit_page");
        let a = seed_menu(&db, &format!("{kw}a"), 1, None).await;
        let b = seed_menu(&db, &format!("{kw}b"), 1, None).await;

        let base = chrono::Local::now().naive_local();
        // update_many 盖不同的审计人与时间：避免为测试改各域 seed 夹具
        use sea_orm::sea_query::Expr;
        for (row, by, offset) in [(a.id, 7_i64, -10), (b.id, 8_i64, -5)] {
            sys_menu::Entity::update_many()
                .filter(sys_menu::Column::Id.eq(row))
                .col_expr(sys_menu::Column::CreatedBy, Expr::value(by))
                .col_expr(sys_menu::Column::UpdatedBy, Expr::value(by))
                .col_expr(
                    sys_menu::Column::CreatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .col_expr(
                    sys_menu::Column::UpdatedAt,
                    Expr::value(base + chrono::Duration::seconds(offset)),
                )
                .exec(&db)
                .await
                .unwrap();
        }

        let all = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(all.total, 2, "前置：keyword 应命中两行");
        let by_creator = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                created_by: Some(7),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_creator.total, 1, "created_by=7 应只命中 a");
        let by_updater = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                updated_by: Some(8),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_updater.total, 1, "updated_by=8 应只命中 b");
        let ghost = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                created_by: Some(9_999_999_999),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(ghost.total, 0, "不存在的创建人应过滤为空");
        let created_after = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                created_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(created_after.total, 1, "begin=base-7s 应只剩 b（晚于阈值）");
        let created_before = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                created_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(created_before.total, 1, "end=base-7s 应只剩 a（早于阈值）");
        let updated_after = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                updated_at_begin: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            updated_after.total, 1,
            "updated_at 范围与 created_at 同机制"
        );
        let updated_before = find_page(
            &db,
            &MenuFilter {
                keyword: Some(kw.clone()),
                updated_at_end: Some(base - chrono::Duration::seconds(7)),
                ..Default::default()
            },
            0,
            10,
        )
        .await
        .unwrap();
        assert_eq!(updated_before.total, 1);

        sys_menu::Entity::delete_many()
            .filter(sys_menu::Column::Id.is_in([a.id, b.id]))
            .exec(&db)
            .await
            .unwrap();
    }

    /// 脏数据自环（`A.parent_id = A`）：级联软删的 BFS 必须终止，不得无限扩展挂死。
    ///
    /// MySQL 不允许 CHECK 引用自增列（错误 3818），故自环无法在 DB 层拦截；这条路径
    /// 只能靠 `visited` 兜底 —— 正是本用例守住的防线。
    #[tokio::test]
    async fn soft_delete_menu_terminates_on_self_referencing_menu() {
        let txn = test_txn().await;
        let a = seed_menu(&txn, &unique("self_loop"), 1, None).await;
        // 绕过 service 防环直接改库造自环
        let mut dirty: sys_menu::ActiveModel = a.clone().into();
        dirty.parent_id = Set(a.id);
        dirty.update(&txn).await.unwrap();

        // 旧实现：frontier 每轮查回自身、child_ids 永不为空 ⇒ 永久循环并占住事务连接
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            soft_delete_menu_in_tx(&txn, a.id),
        )
        .await;

        assert!(result.is_ok(), "自环脏数据下 BFS 未在限时内终止（挂死）");
        assert!(
            find_by_id(&txn, a.id).await.unwrap().is_none(),
            "自环菜单本身应被软删"
        );
    }

    /// 脏数据双节点环（`A→B→A`）：删除 A 必须终止，且环上节点全部级联软删。
    #[tokio::test]
    async fn soft_delete_menu_terminates_on_two_node_cycle() {
        let txn = test_txn().await;
        let a = seed_menu(&txn, &unique("cycle_a"), 1, None).await;
        let b = seed_menu_with_parent(&txn, &unique("cycle_b"), a.id).await;
        // 造环：把 a 的父指向 b，形成 a→b→a
        let mut dirty: sys_menu::ActiveModel = a.clone().into();
        dirty.parent_id = Set(b.id);
        dirty.update(&txn).await.unwrap();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            soft_delete_menu_in_tx(&txn, a.id),
        )
        .await;

        assert!(
            result.is_ok(),
            "双节点环脏数据下 BFS 未在限时内终止（挂死）"
        );
        assert!(
            find_by_id(&txn, a.id).await.unwrap().is_none(),
            "A 应被软删"
        );
        assert!(
            find_by_id(&txn, b.id).await.unwrap().is_none(),
            "B 应被软删"
        );
    }
}
