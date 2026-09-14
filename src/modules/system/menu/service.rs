//! 菜单域业务：vben 菜单树构建。

use std::collections::HashMap;

use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::sys_menu;
use crate::modules::system::menu::dto::{
    CreateMenuReq, MenuFilter, MenuListReq, UpdateMenuReq, VbenMenuItem, VbenMenuMeta,
};
use crate::modules::system::menu::repo as menu_repo;
use crate::modules::system::permission::SUPER_ROLE_KEY;
use crate::modules::system::user::repo as user_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 菜单树最大深度：防御异常数据（超深 parent 链）导致递归栈溢出。
const MAX_MENU_DEPTH: usize = 10;

/// 级联删除的请求级超时（秒）。级联收集虽已用 `visited` 保证迭代有界，但超大子树
/// 或库侧劣化仍可能让单次删除长时间占住连接；超时兜底保证单请求不会拖死连接池。
/// 取值远大于正常级联（数千节点也只有数百次查询），只拦截病态情况。
const DELETE_MENU_TIMEOUT_SECS: u64 = 10;

/// vben 菜单树（`/user/menus`）：超管返回全量，普通用户按角色过滤。
pub async fn get_menus(
    db: &impl ConnectionTrait,
    user_id: u64,
) -> Result<Vec<VbenMenuItem>, AppError> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;
    let is_super = roles.iter().any(|r| r.role_key == SUPER_ROLE_KEY);

    let menus = if is_super {
        menu_repo::find_all_menus(db).await?
    } else {
        // 角色解析（有效角色 = 启用且未删）在 service 组合，repo 只按 role_ids 查询
        let role_ids: Vec<u64> = roles.iter().map(|r| r.id).collect();
        menu_repo::find_menus_by_role_ids(db, &role_ids).await?
    };
    Ok(build_menu_tree(menus))
}

/// sys_menu（parent_id 树）→ vben 菜单树：按钮（menu_type=3）不进菜单树，只进权限码。
fn build_menu_tree(menus: Vec<sys_menu::Model>) -> Vec<VbenMenuItem> {
    let nodes: Vec<sys_menu::Model> = menus.into_iter().filter(|m| m.menu_type != 3).collect();
    let mut by_parent: HashMap<u64, Vec<sys_menu::Model>> = HashMap::new();
    for m in nodes {
        by_parent.entry(m.parent_id).or_default().push(m);
    }

    // 递归构建子树；depth 超过 MAX_MENU_DEPTH 时截断（防超深异常数据栈溢出）
    fn build(
        parent_id: u64,
        by_parent: &HashMap<u64, Vec<sys_menu::Model>>,
        depth: usize,
    ) -> Vec<VbenMenuItem> {
        if depth > MAX_MENU_DEPTH {
            return Vec::new();
        }
        by_parent
            .get(&parent_id)
            .map(|items| {
                items
                    .iter()
                    .map(|m| VbenMenuItem {
                        path: m.path.clone(),
                        name: m.name.clone(),
                        component: m.component.clone(),
                        meta: VbenMenuMeta {
                            title: m.title.clone(),
                            icon: m.icon.clone(),
                            order: m.sort,
                            keep_alive: m.keep_alive == 1,
                            hide_in_menu: m.hidden == 1,
                        },
                        children: build(m.id, by_parent, depth + 1),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    build(0, &by_parent, 1)
}

/// 菜单分页查询：请求参数（keyword / status / menu_type / 审计过滤）组装为 repo 过滤条件。
pub async fn page_menus(
    db: &impl ConnectionTrait,
    req: &MenuListReq,
) -> Result<PageData<sys_menu::Model>, AppError> {
    let model = menu_repo::find_page(
        db,
        &MenuFilter {
            keyword: req.keyword.clone(),
            status: req.status,
            menu_type: req.menu_type,
            created_by: req.created_by,
            updated_by: req.updated_by,
            created_at_begin: crate::utils::datetime::parse_datetime(
                "createdAtBegin",
                &req.created_at_begin,
                false,
            )?,
            created_at_end: crate::utils::datetime::parse_datetime(
                "createdAtEnd",
                &req.created_at_end,
                true,
            )?,
            updated_at_begin: crate::utils::datetime::parse_datetime(
                "updatedAtBegin",
                &req.updated_at_begin,
                false,
            )?,
            updated_at_end: crate::utils::datetime::parse_datetime(
                "updatedAtEnd",
                &req.updated_at_end,
                true,
            )?,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(model)
}

/// 创建菜单：name 查重（含软删占位）→ 落库（审计字段由 repo 盖章）。
/// component 格式等值域校验已前移到 handler 前的 `menu::validate`。
///
/// 对外入口：开事务后委托 `create_menu_in_tx`，成功后提交——父存在性校验与落库必须原子，
/// 否则并发下会把子菜单挂到刚被删掉的父菜单下。
pub async fn create_menu(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreateMenuReq,
) -> Result<sys_menu::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_menu_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内创建菜单：父存在性校验（加锁读）→ name 查重 → 落库。
pub(crate) async fn create_menu_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateMenuReq,
) -> Result<sys_menu::Model, AppError> {
    // 父菜单校验：parent_id = 0 为顶级节点，否则须存在且未软删。
    // 加锁读：与并发移动/删除该父的事务在父行上互斥，避免挂到刚被删的父下。
    if req.parent_id != 0
        && menu_repo::find_by_id_for_update(txn, req.parent_id)
            .await?
            .is_none()
    {
        return Err(AppError::Biz("上级菜单不存在或已删除".to_string()));
    }

    // 检查名称是否存在（name 全局唯一；并发重复由唯一键兜底，这里给友好错误）
    let menu = menu_repo::find_by_name_include_deleted(txn, &req.name).await?;
    if let Some(menu) = menu {
        return Err(AppError::Biz(format!("菜单名称已存在：{}", menu.name)));
    }

    // 创建菜单
    let menu = menu_repo::create_menu(
        txn,
        sys_menu::ActiveModel {
            parent_id: Set(req.parent_id),
            path: Set(req.path.clone()),
            name: Set(req.name.clone()),
            component: Set(req.component.clone()),
            title: Set(req.title.clone()),
            icon: Set(req.icon.clone()),
            sort: Set(req.sort),
            keep_alive: Set(req.keep_alive),
            hidden: Set(req.hidden),
            menu_type: Set(req.menu_type),
            permission: Set(req.permission.clone()),
            status: Set(req.status),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(menu)
}

/// 换父前的行锁：按 id 升序对「被移动节点」与「新父」加排他锁，返回两者最新记录。
///
/// 升序是防死锁的关键：`A 移到 B 下` 与 `B 移到 A 下` 两个方向相反的并发请求加锁顺序
/// 一致，不会交叉等待（与 dept 域 `lock_move_rows` 同款语义）。`parent_id = 0` 表示移到
/// 顶级，无父行可锁；`None` 表示目标行不存在或已软删，文案由调用方决定。
async fn lock_move_rows(
    txn: &DatabaseTransaction,
    menu_id: u64,
    parent_id: u64,
) -> Result<(Option<sys_menu::Model>, Option<sys_menu::Model>), AppError> {
    if parent_id == 0 {
        let menu = menu_repo::find_by_id_for_update(txn, menu_id).await?;
        return Ok((menu, None));
    }

    // 小 id 先锁（自环场景 parent_id == menu_id 是同一行，重复加锁无副作用）
    if parent_id <= menu_id {
        let parent = menu_repo::find_by_id_for_update(txn, parent_id).await?;
        let menu = menu_repo::find_by_id_for_update(txn, menu_id).await?;
        Ok((menu, parent))
    } else {
        let menu = menu_repo::find_by_id_for_update(txn, menu_id).await?;
        let parent = menu_repo::find_by_id_for_update(txn, parent_id).await?;
        Ok((menu, parent))
    }
}

/// 更新菜单：判存在 → 防环 → name 查重排除自身 → 全量覆盖（审计字段由 repo 盖章）。
///
/// 对外入口：开事务后委托 `update_menu_in_tx`，成功后提交——防环校验与换父必须原子。
pub async fn update_menu(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateMenuReq,
) -> Result<sys_menu::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_menu_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内更新菜单：被移动节点与新父都走加锁读，防环判定基于最新已提交结构。
pub(crate) async fn update_menu_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateMenuReq,
) -> Result<sys_menu::Model, AppError> {
    // 加锁读（升序）：防环是「读-判-写」不变式，快照读下两个方向相反的并发互移会各自
    // 只看到旧结构、双双通过校验，最终互指成环
    let (menu, parent) = lock_move_rows(txn, req.id, req.parent_id).await?;

    // 检查菜单是否存在（软删视为不存在）
    let Some(_) = menu else {
        return Err(AppError::Biz(format!("菜单不存在：{}", req.id)));
    };

    // 父菜单校验：0 为顶级；否则须存在，且不得为自身或其子孙（防环）
    if req.parent_id != 0 {
        // 自环单独拦截：子孙集合不含自身，contains 判不出来
        if req.parent_id == req.id {
            return Err(AppError::Biz("上级菜单不能是自身或其下级菜单".to_string()));
        }
        if parent.is_none() {
            return Err(AppError::Biz("上级菜单不存在或已删除".to_string()));
        }
        // 子孙集合也用加锁读收集：必须看到并发已提交的换父结果
        if menu_repo::find_descendant_ids_for_update(txn, req.id)
            .await?
            .contains(&req.parent_id)
        {
            return Err(AppError::Biz("上级菜单不能是自身或其下级菜单".to_string()));
        }
    }

    // 检查名称是否已被其他菜单占用（含软删占位，排除自身）
    let dup = menu_repo::find_by_name_include_deleted(txn, &req.name)
        .await?
        .is_some_and(|existing| existing.id != req.id);
    if dup {
        return Err(AppError::Biz("菜单名称已存在".to_string()));
    }

    // 更新菜单
    let menu = menu_repo::update_menu(
        txn,
        sys_menu::ActiveModel {
            id: Set(req.id),
            parent_id: Set(req.parent_id),
            path: Set(req.path.clone()),
            name: Set(req.name.clone()),
            component: Set(req.component.clone()),
            title: Set(req.title.clone()),
            icon: Set(req.icon.clone()),
            sort: Set(req.sort),
            keep_alive: Set(req.keep_alive),
            hidden: Set(req.hidden),
            menu_type: Set(req.menu_type),
            permission: Set(req.permission.clone()),
            status: Set(req.status),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(menu)
}

/// 查询单个菜单详情（排除软删除）；不存在返回业务错误。
pub async fn get_menu(db: &impl ConnectionTrait, id: u64) -> Result<sys_menu::Model, AppError> {
    let Some(menu) = menu_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("菜单不存在：{id}")));
    };
    Ok(menu)
}

/// 批量按 id 查有效菜单并加锁（跨域引用校验用），空入参返回空数组。
///
/// 与 `menu_repo::find_by_id` 的区别是**加了排他锁**：跨域写入（role 域写
/// `sys_role_menu`）必须先锁住被引用的菜单行，才能与「删除菜单」的级联清理串行化
/// ——删除方清完关联后绑定方才插入，会留下指向已软删菜单的悬挂绑定。须在事务内调用。
pub async fn find_by_ids_for_update(
    txn: &DatabaseTransaction,
    ids: &[u64],
) -> Result<Vec<sys_menu::Model>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(menu_repo::find_by_ids_for_update(txn, ids).await?)
}

/// 对外入口：开事务后委托 `delete_menu_in_tx`，成功后提交。
///
/// 超时兜底：超过 `DELETE_MENU_TIMEOUT_SECS` 未完成即放弃本次删除——future 在 `await`
/// 点被丢弃，`txn` 随之 Drop 触发回滚（沿用本仓既有的事务 Drop 语义），连接立即归还池，
/// 单个请求无法长期独占连接。返回业务错误而非 `Internal`：超时对调用方是可行动信息
/// （树层级异常 / 库侧劣化），且与本仓「Biz 文案面向用户、Internal 只回固定串」的分工一致。
pub async fn delete_menu(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(DELETE_MENU_TIMEOUT_SECS),
        delete_menu_in_tx(&txn, id),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            return Err(AppError::Biz(format!(
                "删除菜单超时（超过 {DELETE_MENU_TIMEOUT_SECS} 秒）"
            )));
        }
    };
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（存在性 + 级联软删子孙菜单并清空角色关联），不管理事务边界。
/// 供对外入口与测试外层事务调用。
///
/// 存在性检查走加锁读：与并发移动/删除本菜单的事务在自身行上互斥（与 dept 域删除同款）。
pub(crate) async fn delete_menu_in_tx(txn: &DatabaseTransaction, id: u64) -> Result<(), AppError> {
    // 检查菜单是否存在（软删视为不存在）
    let Some(_) = menu_repo::find_by_id_for_update(txn, id).await? else {
        return Err(AppError::Biz("菜单不存在".to_string()));
    };

    menu_repo::soft_delete_menu_in_tx(txn, id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_menu, sys_role, sys_role_menu, sys_user, sys_user_role};
    use crate::modules::system::menu::dto::{CreateMenuReq, UpdateMenuReq};
    use crate::utils::error::AppError;
    use chrono::Local;
    use sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use std::collections::HashSet;
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
    ///
    /// 注意：`create_menu` / `update_menu` 已是自开事务的对外入口（收 `&DatabaseConnection`），
    /// 域内测试统一调用 `_in_tx` 版本以复用本夹具的回滚隔离。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    fn create_req(name: String, component: String) -> CreateMenuReq {
        CreateMenuReq {
            parent_id: 0,
            path: format!("/{name}"),
            name,
            component,
            title: unique("title"),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
        }
    }

    /// 更新请求夹具：默认顶级（parent_id = 0），需要时由用例覆写。
    fn update_req(id: u64, name: String) -> UpdateMenuReq {
        UpdateMenuReq {
            id,
            parent_id: 0,
            path: format!("/{name}"),
            name,
            component: format!("#/views/{}.vue", unique("comp")),
            title: unique("title"),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
        }
    }

    async fn seed_menu(
        db: &impl ConnectionTrait,
        name: &str,
        parent_id: u64,
        menu_type: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
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
            menu_type: Set(menu_type),
            permission: Set(String::new()),
            status: Set(1),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_user(db: &impl ConnectionTrait) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique("menu_user")),
            password: Set("x".to_string()),
            nickname: Set("菜单测试用户".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(db: &impl ConnectionTrait, role_key: &str) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("menu_role")),
            role_key: Set(role_key.to_string()),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 加载全局唯一的 super 角色：已存在则复用（测后不清理），由本测试创建才清理。
    async fn load_super_role(db: &impl ConnectionTrait) -> (sys_role::Model, bool) {
        let existing = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq(SUPER_ROLE_KEY))
            .one(db)
            .await
            .unwrap();
        if let Some(role) = existing {
            return (role, false);
        }
        let role = seed_role(db, SUPER_ROLE_KEY).await;
        (role, true)
    }

    async fn bind_user_role(db: &impl ConnectionTrait, user_id: u64, role_id: u64) {
        sys_user_role::ActiveModel {
            user_id: Set(user_id),
            role_id: Set(role_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn bind_role_menu(db: &impl ConnectionTrait, role_id: u64, menu_id: u64) {
        sys_role_menu::ActiveModel {
            role_id: Set(role_id),
            menu_id: Set(menu_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// 清理顺序：先删关联表，再删主表。
    async fn cleanup(
        db: &impl ConnectionTrait,
        user_ids: &[u64],
        role_ids: &[u64],
        menu_ids: &[u64],
    ) {
        for user_id in user_ids {
            sys_user_role::Entity::delete_many()
                .filter(sys_user_role::Column::UserId.eq(*user_id))
                .exec(db)
                .await
                .unwrap();
        }
        for role_id in role_ids {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(*role_id))
                .exec(db)
                .await
                .unwrap();
        }
        sys_user::Entity::delete_many()
            .filter(sys_user::Column::Id.is_in(user_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_many()
            .filter(sys_role::Column::Id.is_in(role_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
        sys_menu::Entity::delete_many()
            .filter(sys_menu::Column::Id.is_in(menu_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    /// 创建时 name 重复（含软删占位）应被业务层拒绝。
    #[tokio::test]
    async fn create_menu_rejects_duplicate_name_including_soft_deleted() {
        let db = test_txn().await;
        let deleted_name = unique("dup_deleted");
        let live_name = unique("dup_live");
        let _deleted = seed_menu(
            &db,
            &deleted_name,
            0,
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let _live = seed_menu(&db, &live_name, 0, 1, None).await;

        let result_deleted = create_menu_in_tx(
            &db,
            ACTOR_ID,
            &create_req(deleted_name, format!("#/views/{}.vue", unique("c"))),
        )
        .await;
        let result_live = create_menu_in_tx(
            &db,
            ACTOR_ID,
            &create_req(live_name, format!("#/views/{}.vue", unique("c"))),
        )
        .await;

        assert!(
            matches!(result_deleted, Err(AppError::Biz(_))),
            "软删菜单占用的 name 应被拒绝，实际：{result_deleted:?}"
        );
        assert!(
            matches!(result_live, Err(AppError::Biz(_))),
            "正常菜单占用的 name 应被拒绝，实际：{result_live:?}"
        );
    }

    /// 创建菜单：父菜单不存在（悬空 parent_id）应被拒绝，避免产生孤儿菜单。
    #[tokio::test]
    async fn create_menu_rejects_nonexistent_parent() {
        let db = test_txn().await;
        let mut req = create_req(unique("orphan"), format!("#/views/{}.vue", unique("c")));
        req.parent_id = 9_999_999_999;

        let result = create_menu_in_tx(&db, ACTOR_ID, &req).await;

        assert!(
            matches!(result, Err(AppError::Biz(_))),
            "父菜单不存在应返回 Biz 业务错误，实际：{result:?}"
        );
    }

    /// 创建菜单：`parent_id = 0` 为顶级节点，应正常通过（防过度拦截）。
    #[tokio::test]
    async fn create_menu_accepts_root_parent_zero() {
        let db = test_txn().await;
        let req = create_req(unique("root_ok"), format!("#/views/{}.vue", unique("c")));
        assert_eq!(req.parent_id, 0, "前置：create_req 默认顶级菜单");

        let created = create_menu_in_tx(&db, ACTOR_ID, &req).await.unwrap();

        assert_eq!(created.parent_id, 0);
    }

    /// 更新时 name 与他人重复应被拒绝，但保留自身 name 不算重复。
    #[tokio::test]
    async fn update_menu_rejects_duplicate_name_excluding_self() {
        let db = test_txn().await;
        let name_a = unique("menu_a");
        let name_b = unique("menu_b");
        let _menu_a = seed_menu(&db, &name_a, 0, 1, None).await;
        let menu_b = seed_menu(&db, &name_b, 0, 1, None).await;

        let update_req = |name: String| UpdateMenuReq {
            id: menu_b.id,
            parent_id: 0,
            path: format!("/{name}"),
            name,
            component: format!("#/views/{}.vue", unique("comp")),
            title: unique("title"),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
        };

        let dup = update_menu_in_tx(&db, ACTOR_ID, &update_req(name_a.clone())).await;
        let keep_self = update_menu_in_tx(&db, ACTOR_ID, &update_req(name_b.clone())).await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 name 应返回 Biz 业务错误，实际：{dup:?}"
        );
        let updated = keep_self.expect("保留自身 name 应更新成功");
        assert_eq!(updated.name, name_b);
    }

    /// 更新不存在的菜单（或已软删菜单）应返回业务错误。
    #[tokio::test]
    async fn update_menu_returns_biz_error_when_menu_missing() {
        let db = test_txn().await;

        let missing = update_menu_in_tx(
            &db,
            ACTOR_ID,
            &UpdateMenuReq {
                id: 9_999_999_999,
                parent_id: 0,
                path: "/missing".to_string(),
                name: unique("missing"),
                component: format!("#/views/{}.vue", unique("comp")),
                title: unique("title"),
                icon: String::new(),
                sort: 0,
                keep_alive: 0,
                hidden: 0,
                menu_type: 1,
                permission: String::new(),
                status: 1,
            },
        )
        .await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "更新不存在的菜单应返回 Biz 业务错误，实际：{missing:?}"
        );

        let deleted_name = unique("deleted_menu");
        let deleted = seed_menu(
            &db,
            &deleted_name,
            0,
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let update_deleted = update_menu_in_tx(
            &db,
            ACTOR_ID,
            &UpdateMenuReq {
                id: deleted.id,
                parent_id: 0,
                path: "/deleted".to_string(),
                name: deleted_name,
                component: format!("#/views/{}.vue", unique("comp")),
                title: unique("title"),
                icon: String::new(),
                sort: 0,
                keep_alive: 0,
                hidden: 0,
                menu_type: 1,
                permission: String::new(),
                status: 1,
            },
        )
        .await;
        assert!(
            matches!(update_deleted, Err(AppError::Biz(_))),
            "更新已软删菜单应返回 Biz 业务错误，实际：{update_deleted:?}"
        );
    }

    /// 更新菜单：`parent_id` 指向自身（自环）应被拒绝。
    #[tokio::test]
    async fn update_menu_rejects_self_as_parent() {
        let db = test_txn().await;
        let menu = seed_menu(&db, &unique("self_parent"), 0, 1, None).await;

        let mut req = update_req(menu.id, unique("self_parent_renamed"));
        req.parent_id = menu.id;
        let result = update_menu_in_tx(&db, ACTOR_ID, &req).await;

        assert!(
            matches!(result, Err(AppError::Biz(_))),
            "parent_id 指向自身应返回 Biz 业务错误，实际：{result:?}"
        );
    }

    /// 更新菜单：把节点移动到自身下级（成环）应被拒绝。
    #[tokio::test]
    async fn update_menu_rejects_move_under_own_descendant() {
        let db = test_txn().await;
        let parent = seed_menu(&db, &unique("cycle_parent"), 0, 1, None).await;
        let child = seed_menu(&db, &unique("cycle_child"), parent.id, 1, None).await;

        let mut req = update_req(parent.id, unique("cycle_parent_renamed"));
        req.parent_id = child.id;
        let result = update_menu_in_tx(&db, ACTOR_ID, &req).await;

        assert!(
            matches!(result, Err(AppError::Biz(_))),
            "移动到自身下级（成环）应返回 Biz 业务错误，实际：{result:?}"
        );
    }

    /// 更新菜单：父菜单不存在应被拒绝。
    #[tokio::test]
    async fn update_menu_rejects_nonexistent_parent() {
        let db = test_txn().await;
        let menu = seed_menu(&db, &unique("orphan_update"), 0, 1, None).await;

        let mut req = update_req(menu.id, unique("orphan_update_renamed"));
        req.parent_id = 9_999_999_999;
        let result = update_menu_in_tx(&db, ACTOR_ID, &req).await;

        assert!(
            matches!(result, Err(AppError::Biz(_))),
            "父菜单不存在应返回 Biz 业务错误，实际：{result:?}"
        );
    }

    /// 更新菜单：移动到无关分支或顶级是合法操作（防过度拦截）。
    #[tokio::test]
    async fn update_menu_allows_move_to_other_branch() {
        let db = test_txn().await;
        let branch_a = seed_menu(&db, &unique("move_branch_a"), 0, 1, None).await;
        let branch_b = seed_menu(&db, &unique("move_branch_b"), 0, 1, None).await;
        let node = seed_menu(&db, &unique("move_node"), branch_a.id, 1, None).await;

        let mut to_other = update_req(node.id, unique("move_node_other"));
        to_other.parent_id = branch_b.id;
        let moved = update_menu_in_tx(&db, ACTOR_ID, &to_other).await.unwrap();

        assert_eq!(moved.parent_id, branch_b.id, "移动到无关分支应成功");

        let mut to_root = update_req(node.id, unique("move_node_root"));
        to_root.parent_id = 0;
        let rooted = update_menu_in_tx(&db, ACTOR_ID, &to_root).await.unwrap();

        assert_eq!(rooted.parent_id, 0, "移动到顶级应成功");
    }

    /// 删除不存在的菜单应返回业务错误。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn delete_menu_returns_biz_error_when_menu_missing() {
        let txn = test_txn().await;

        let missing = delete_menu_in_tx(&txn, 9_999_999_999).await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "删除不存在的菜单应返回 Biz 业务错误，实际：{missing:?}"
        );
    }

    /// 对外入口 `delete_menu`（自管事务 + 超时包装）正常路径：级联生效并提交。
    /// 走真实连接而非外层事务：超时包装若误吞 commit，本用例会红。
    #[tokio::test]
    async fn delete_menu_commits_cascade_via_public_entry() {
        let db = test_db().await;
        let parent = seed_menu(&db, &unique("pub_del_parent"), 0, 1, None).await;
        let child = seed_menu(&db, &unique("pub_del_child"), parent.id, 1, None).await;

        delete_menu(&db, parent.id).await.unwrap();

        assert!(
            menu_repo::find_by_id(&db, parent.id)
                .await
                .unwrap()
                .is_none(),
            "父菜单应在提交后软删"
        );
        assert!(
            menu_repo::find_by_id(&db, child.id)
                .await
                .unwrap()
                .is_none(),
            "子菜单应被级联软删"
        );

        cleanup(&db, &[], &[], &[parent.id, child.id]).await;
    }

    /// 普通用户只看到其角色绑定的菜单（父子结构完整），未绑定菜单不出现。
    #[tokio::test]
    async fn get_menus_regular_user_filters_by_role_bindings() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let role = seed_role(&db, &unique("regular_key")).await;
        bind_user_role(&db, user.id, role.id).await;
        let parent = seed_menu(&db, &unique("bind_parent"), 0, 1, None).await;
        let child = seed_menu(&db, &unique("bind_child"), parent.id, 1, None).await;
        let other = seed_menu(&db, &unique("unbound"), 0, 1, None).await;
        bind_role_menu(&db, role.id, parent.id).await;
        bind_role_menu(&db, role.id, child.id).await;

        let tree = get_menus(&db, user.id).await.unwrap();

        cleanup(
            &db,
            &[user.id],
            &[role.id],
            &[parent.id, child.id, other.id],
        )
        .await;

        assert_eq!(tree.len(), 1, "普通用户只应看到绑定的顶级菜单");
        assert_eq!(tree[0].name, parent.name);
        assert_eq!(tree[0].children.len(), 1, "绑定子菜单应保留层级");
        assert_eq!(tree[0].children[0].name, child.name);
    }

    /// 超管返回全量菜单树（未绑定也可见），按钮（menu_type=3）不进树。
    #[tokio::test]
    async fn get_menus_super_returns_full_tree_without_buttons() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let (super_role, owned_by_test) = load_super_role(&db).await;
        bind_user_role(&db, user.id, super_role.id).await;
        let page = seed_menu(&db, &unique("super_page"), 0, 1, None).await;
        let button = seed_menu(&db, &unique("super_button"), page.id, 3, None).await;

        let tree = get_menus(&db, user.id).await.unwrap();

        let _role_ids: &[u64] = if owned_by_test { &[super_role.id] } else { &[] };

        assert!(
            contains_name(&tree, &page.name),
            "超管树应包含创建的页面（全量菜单）"
        );
        assert!(
            !contains_name(&tree, &button.name),
            "按钮（menu_type=3）不应进入菜单树"
        );
    }

    /// 递归判断菜单树中是否存在指定 name 的节点。
    fn contains_name(nodes: &[VbenMenuItem], name: &str) -> bool {
        nodes
            .iter()
            .any(|n| n.name == name || contains_name(&n.children, name))
    }

    /// 构造单条菜单记录（menu_type=1 目录/页面）。
    fn menu(id: u64, parent_id: u64) -> sys_menu::Model {
        sys_menu::Model {
            id,
            parent_id,
            path: format!("/m{id}"),
            name: format!("m{id}"),
            component: format!("#/views/m{id}.vue"),
            title: format!("M{id}"),
            icon: String::new(),
            sort: 0,
            keep_alive: 0,
            hidden: 0,
            menu_type: 1,
            permission: String::new(),
            status: 1,
            created_at: Local::now().naive_local(),
            updated_at: Local::now().naive_local(),
            created_by: 0,
            updated_by: 0,
            deleted_at: None,
        }
    }

    /// 超深链（异常数据）不应导致递归栈溢出：超过深度上限的层级被截断。
    #[test]
    fn build_menu_tree_truncates_abnormal_deep_chain() {
        // 60 层单链：1 ← 2 ← ... ← 60（parent 指向上层）
        let menus: Vec<sys_menu::Model> = (1..=60).map(|id| menu(id, id - 1)).collect();

        let tree = build_menu_tree(menus);

        // 第一层只有一个根节点，深度被限制后最深节点的 children 为空
        assert_eq!(tree.len(), 1);
        let mut node = &tree[0];
        let mut depth = 1;
        while !node.children.is_empty() {
            node = &node.children[0];
            depth += 1;
        }
        assert!(
            depth <= MAX_MENU_DEPTH,
            "异常深链应被截断到深度上限，实际深度：{depth}"
        );
    }

    /// 从根（`parent_id = 0`）沿 `parent_id` 遍历，判断目标菜单是否仍挂在树上。
    fn reachable_from_root(all: &[sys_menu::Model], target: u64) -> bool {
        let mut frontier = vec![0_u64];
        let mut visited = HashSet::new();
        while let Some(parent_id) = frontier.pop() {
            for menu in all.iter().filter(|menu| menu.parent_id == parent_id) {
                if menu.id == target {
                    return true;
                }
                if visited.insert(menu.id) {
                    frontier.push(menu.id);
                }
            }
        }
        false
    }

    /// 并发互移防环：事务 1「A 移到 B 下」与事务 2「B 移到 A 下」各自校验时都看不到对方的
    /// 未提交改动（RR 快照读），两边都放行即互指成环，两个菜单从树中消失。
    ///
    /// 复现要点：事务 2 先读一次固定快照，事务 1 完整提交后事务 2 才继续。
    /// 数据真实提交（并发必需），故用真连接 + 手工清理。
    #[tokio::test]
    async fn update_menu_concurrent_cross_move_cannot_create_cycle() {
        let db = test_db().await;
        let root = seed_menu(&db, &unique("cycle_root"), 0, 1, None).await;
        let a = seed_menu(&db, &unique("cycle_a"), root.id, 1, None).await;
        let b = seed_menu(&db, &unique("cycle_b"), root.id, 1, None).await;

        // 事务 2 先开启并做一次快照读（此后它的普通读都停留在这一刻）
        let txn2 = db.begin().await.unwrap();
        assert!(
            menu_repo::find_by_id(&txn2, a.id).await.unwrap().is_some(),
            "前置：A 应存在"
        );

        // 事务 1：A 移到 B 下，提交
        let txn1 = db.begin().await.unwrap();
        let mut move_a = update_req(a.id, a.name.clone());
        move_a.parent_id = b.id;
        let moved_a = update_menu_in_tx(&txn1, ACTOR_ID, &move_a).await;
        assert!(moved_a.is_ok(), "前置：单向移动应放行: {moved_a:?}");
        txn1.commit().await.unwrap();

        // 事务 2：B 移到 A 下——A 已是 B 的父，必须被防环拒绝
        let mut move_b = update_req(b.id, b.name.clone());
        move_b.parent_id = a.id;
        let res = update_menu_in_tx(&txn2, ACTOR_ID, &move_b).await;
        if res.is_ok() {
            txn2.commit().await.unwrap();
        } else {
            txn2.rollback().await.unwrap();
        }

        // 用户可见症状：成环后两个菜单都不再从根可达
        let all = menu_repo::find_all_menus(&db).await.unwrap();
        let reachable = (
            reachable_from_root(&all, a.id),
            reachable_from_root(&all, b.id),
        );
        cleanup(&db, &[], &[], &[root.id, a.id, b.id]).await;

        assert!(
            matches!(res, Err(AppError::Biz(ref m)) if m.contains("自身或其下级")),
            "并发互移必须拒绝其一，否则 A/B 互指成环: {res:?}"
        );
        assert_eq!(
            reachable,
            (true, true),
            "A/B 都应仍能从根遍历到（成环则双双从树中消失）"
        );
    }

    /// 真并发冒烟：两个方向相反的互移同时发起，验证加锁顺序不交叉等待（无死锁/锁等待超时）。
    ///
    /// 只断言与调度无关的不变量（至少一方被拒、无环）；哪一方胜出由抢锁顺序决定。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_cross_moves_reject_one_side_without_deadlock() {
        let db = test_db().await;
        let root = seed_menu(&db, &unique("smoke_root"), 0, 1, None).await;
        let a = seed_menu(&db, &unique("smoke_a"), root.id, 1, None).await;
        let b = seed_menu(&db, &unique("smoke_b"), root.id, 1, None).await;

        let (db1, db2) = (db.clone(), db.clone());
        let (mut req1, mut req2) = (
            update_req(a.id, a.name.clone()),
            update_req(b.id, b.name.clone()),
        );
        req1.parent_id = b.id;
        req2.parent_id = a.id;
        let h1 = tokio::spawn(async move { update_menu(&db1, ACTOR_ID, &req1).await });
        let h2 = tokio::spawn(async move { update_menu(&db2, ACTOR_ID, &req2).await });
        let (r1, r2) = (h1.await.unwrap(), h2.await.unwrap());

        let all = menu_repo::find_all_menus(&db).await.unwrap();
        let reachable = (
            reachable_from_root(&all, a.id),
            reachable_from_root(&all, b.id),
        );
        cleanup(&db, &[], &[], &[root.id, a.id, b.id]).await;

        assert!(
            r1.is_err() || r2.is_err(),
            "并发互移至少一方应被防环拒绝: r1={r1:?} r2={r2:?}"
        );
        assert_eq!(reachable, (true, true), "不应成环");
    }

    /// 并发「建子菜单」+「删父菜单」：级联删除必须覆盖已提交的新建子孙，否则留下孤立菜单。
    #[tokio::test]
    async fn delete_menu_cascades_child_created_after_snapshot() {
        let db = test_db().await;
        let parent = seed_menu(&db, &unique("cascade_parent"), 0, 1, None).await;

        // 事务 2 先固定快照（此后它的普通读停在「建子之前」）
        let txn2 = db.begin().await.unwrap();
        assert!(
            menu_repo::find_by_id(&txn2, parent.id)
                .await
                .unwrap()
                .is_some(),
            "前置：父菜单应存在"
        );

        // 事务 1：在父菜单下建子，提交
        let txn1 = db.begin().await.unwrap();
        let mut child_req = create_req(unique("cascade_child"), unique("cascade_comp"));
        child_req.parent_id = parent.id;
        let child = create_menu_in_tx(&txn1, ACTOR_ID, &child_req)
            .await
            .unwrap();
        txn1.commit().await.unwrap();

        // 事务 2：删除父菜单并提交（级联软删要落库才能验证）
        let res = delete_menu_in_tx(&txn2, parent.id).await;
        txn2.commit().await.unwrap();

        let parent_after = menu_repo::find_by_id(&db, parent.id).await.unwrap();
        let child_after = menu_repo::find_by_id(&db, child.id).await.unwrap();
        cleanup(&db, &[], &[], &[child.id, parent.id]).await;

        assert!(res.is_ok(), "删除应成功: {res:?}");
        assert!(parent_after.is_none(), "父菜单应被软删");
        assert!(
            child_after.is_none(),
            "并发新建的子菜单应被级联软删，而不是留下孤立菜单"
        );
    }

    /// 对外入口确实提交事务（`create_menu` 已改为自开事务的包装）。
    #[tokio::test]
    async fn create_menu_commits_via_public_entry() {
        let db = test_db().await;
        let name = unique("commit_create");
        let created = create_menu(
            &db,
            ACTOR_ID,
            &create_req(name.clone(), unique("commit_comp")),
        )
        .await
        .unwrap();

        let reloaded = menu_repo::find_by_id(&db, created.id).await.unwrap();
        cleanup(&db, &[], &[], &[created.id]).await;

        assert_eq!(created.name, name);
        assert!(reloaded.is_some(), "对外入口应自行提交事务，重新连接可见");
    }

    /// 对外入口确实提交事务（`update_menu` 已改为自开事务的包装）。
    #[tokio::test]
    async fn update_menu_commits_via_public_entry() {
        let db = test_db().await;
        let menu = seed_menu(&db, &unique("commit_update"), 0, 1, None).await;
        let new_name = unique("commit_update_v2");

        let updated = update_menu(&db, ACTOR_ID, &update_req(menu.id, new_name.clone()))
            .await
            .unwrap();

        let reloaded = menu_repo::find_by_id(&db, menu.id).await.unwrap();
        cleanup(&db, &[], &[], &[menu.id]).await;

        assert_eq!(updated.name, new_name);
        assert_eq!(
            reloaded.map(|menu| menu.name),
            Some(new_name),
            "对外入口应自行提交事务，重新连接可见"
        );
    }
}
