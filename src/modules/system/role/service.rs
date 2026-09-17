use crate::modules::system::permission::SUPER_ROLE_KEY;
use crate::modules::system::role::dto::UpdateRoleStatusReq;
use crate::utils::PageData;
use crate::utils::check::duplicate_ids;
use crate::{
    modules::system::{
        menu::service as menu_service,
        role::{
            dto::{CreateRoleReq, RoleFilter, RoleListReq, UpdateRoleReq},
            repo as role_repo,
        },
        sys_api::service as api_service,
    },
    utils::check::{collect_missing_ids, format_ids},
    utils::error::AppError,
};
use sea_orm::{
    ActiveValue::Set, ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait,
};

use crate::entity::sys_role;

/// 绑定前校验菜单 / 接口引用有效，并**锁住这些行**。
///
/// 两件事缺一不可：
/// - **加锁读**：与「删除菜单 / 删除接口」的关联清理在同一行上互斥。删除方已加锁读自身行，
///   这里不锁就会在对方清完关联后插入，留下指向已软删记录的悬挂绑定；
/// - **存在性判定**：加锁读拿到的是最新已提交状态，据此拒绝失效 id（只加锁不判定，
///   已软删的 id 仍会被写进关系表）。
///
/// 缺失文案与 user 域挂载部门的「部门不存在：{}」一致，permission 系统失败宁响不默。
async fn ensure_menus_and_apis_exist(
    txn: &DatabaseTransaction,
    menu_ids: &[u64],
    api_ids: &[u64],
) -> Result<(), AppError> {
    if !menu_ids.is_empty() {
        let duplicates = duplicate_ids(menu_ids);
        if !duplicates.is_empty() {
            return Err(AppError::Biz("菜单重复".to_string()));
        }

        let found = menu_service::find_by_ids_for_update(txn, menu_ids).await?;
        let missing = collect_missing_ids(menu_ids, found.iter().map(|menu| menu.id));
        if !missing.is_empty() {
            return Err(AppError::Biz(format!(
                "菜单不存在：{}",
                format_ids(&missing)
            )));
        }
    }

    if !api_ids.is_empty() {
        let duplicates = duplicate_ids(api_ids);
        if !duplicates.is_empty() {
            return Err(AppError::Biz("接口重复".to_string()));
        }

        let found = api_service::find_by_ids_for_update(txn, api_ids).await?;
        let missing = collect_missing_ids(api_ids, found.iter().map(|api| api.id));
        if !missing.is_empty() {
            return Err(AppError::Biz(format!(
                "接口不存在：{}",
                format_ids(&missing)
            )));
        }
    }

    Ok(())
}

/// 分页查询角色（keyword 模糊匹配 role_name / role_key，status 精确，审计过滤），排除软删除。
pub async fn page_roles(
    db: &impl ConnectionTrait,
    req: &RoleListReq,
) -> Result<PageData<sys_role::Model>, AppError> {
    let model = role_repo::find_page(
        db,
        &RoleFilter {
            keyword: req.keyword.clone(),
            status: req.status,
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

/// 批量按 id 查询有效角色（排除软删除），供权限模块等按 id 集合取角色。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: Vec<u64>,
) -> Result<Vec<sys_role::Model>, AppError> {
    let roles = role_repo::find_by_ids(db, ids).await?;
    Ok(roles)
}

pub async fn ensure_roles_exist(
    txn: &DatabaseTransaction,
    role_ids: &[u64],
) -> Result<(), AppError> {
    if role_ids.is_empty() {
        return Ok(());
    }
    let duplicates = duplicate_ids(role_ids);
    if !duplicates.is_empty() {
        return Err(AppError::Biz("角色重复".to_string()));
    }
    let roles = role_repo::find_by_ids_for_update(txn, role_ids).await?;
    let missing = collect_missing_ids(role_ids, roles.iter().map(|role| role.id));
    if !missing.is_empty() {
        return Err(AppError::Biz(format!(
            "角色不存在：{}",
            format_ids(&missing)
        )));
    }
    Ok(())
}

/// 对外入口：开事务后委托 `create_role_in_tx`，成功后提交。
pub async fn create_role(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreateRoleReq,
) -> Result<sys_role::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_role_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（键/名查重 + 写入），不管理事务边界。供对外入口与测试外层事务调用。
pub(crate) async fn create_role_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateRoleReq,
) -> Result<sys_role::Model, AppError> {
    // 检查角色键是否已存在
    let role = role_repo::find_by_role_key_include_deleted(txn, &req.role_key).await?;
    if role.is_some() {
        return Err(AppError::Biz("角色键已存在".to_string()));
    }
    // 检查角色名称是否已存在
    let role = role_repo::find_by_role_name_include_deleted(txn, &req.role_name).await?;
    if role.is_some() {
        return Err(AppError::Biz("角色名称已存在".to_string()));
    }

    // 绑定前校验并锁定菜单 / 接口引用（拒绝失效 id，与删除方在同一行上互斥）
    ensure_menus_and_apis_exist(txn, &req.menu_ids, &req.api_ids).await?;

    let model = sys_role::ActiveModel {
        role_name: Set(req.role_name.clone()),
        role_key: Set(req.role_key.clone()),
        sort: Set(req.sort),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };

    let model = role_repo::create_role_in_tx(
        txn,
        model,
        req.menu_ids.clone(),
        req.api_ids.clone(),
        actor_id,
    )
    .await?;
    Ok(model)
}

/// 对外入口：开事务后委托 `update_role_in_tx`，成功后提交。
pub async fn update_role(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateRoleReq,
) -> Result<sys_role::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_role_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（存在性 + 超管保护 + 键/名查重 + 写入），不管理事务边界。
/// 供对外入口与测试外层事务调用。
pub(crate) async fn update_role_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateRoleReq,
) -> Result<sys_role::Model, AppError> {
    // 注意：必须先加载库中现有记录，校验其 role_key（而非 req 里的新值），
    // 否则内置超管角色可被改名转移（如改为 other_key 绕过保留字检查）。
    let Some(role) = role_repo::find_by_id(txn, req.id).await? else {
        return Err(AppError::Biz("角色不存在".to_string()));
    };

    // 内置超管角色不允许修改（role_key / 名称 / 状态 / 菜单 / API 关联均冻结）。
    if role.role_key == SUPER_ROLE_KEY {
        return Err(AppError::Biz(
            "系统内置超级管理员角色不允许修改".to_string(),
        ));
    }

    // 保留字：任何普通角色不得改名为内置超管键。
    if req.role_key == SUPER_ROLE_KEY {
        return Err(AppError::Biz(
            "角色键 super 为系统保留字，不允许使用".to_string(),
        ));
    }

    // 检查角色键是否已存在
    let role = role_repo::find_by_role_key_include_deleted(txn, req.role_key.as_str()).await?;
    if let Some(role) = role {
        // 角色键已存在，且不是当前角色键
        if role.id != req.id {
            return Err(AppError::Biz("角色键已存在".to_string()));
        }
    }
    // 检查角色名称是否已存在
    let role = role_repo::find_by_role_name_include_deleted(txn, req.role_name.as_str()).await?;
    if let Some(role) = role {
        // 角色名称已存在，且不是当前角色名称
        if role.id != req.id {
            return Err(AppError::Biz("角色名称已存在".to_string()));
        }
    }

    // 绑定前校验并锁定菜单 / 接口引用（拒绝失效 id，与删除方在同一行上互斥）
    ensure_menus_and_apis_exist(txn, &req.menu_ids, &req.api_ids).await?;

    let model = sys_role::ActiveModel {
        id: Set(req.id),
        role_name: Set(req.role_name.clone()),
        role_key: Set(req.role_key.clone()),
        sort: Set(req.sort),
        status: Set(req.status),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };

    let model = role_repo::update_role_in_tx(
        txn,
        model,
        req.menu_ids.clone(),
        req.api_ids.clone(),
        actor_id,
    )
    .await?;
    Ok(model)
}

/// 查询单个角色详情（排除软删除）；不存在返回业务错误。
pub async fn get_role(db: &impl ConnectionTrait, id: u64) -> Result<sys_role::Model, AppError> {
    let Some(role) = role_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("角色不存在：{id}")));
    };
    Ok(role)
}

/// 对外入口：开事务后委托 `delete_role_in_tx`，成功后提交。
pub async fn delete_role(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_role_in_tx(&txn, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（存在性 + 超管保护 + 软删），不管理事务边界。供对外入口与测试外层事务调用。
pub(crate) async fn delete_role_in_tx(txn: &DatabaseTransaction, id: u64) -> Result<(), AppError> {
    let Some(role) = role_repo::find_by_id(txn, id).await? else {
        return Err(AppError::Biz(format!("角色不存在：{id}")));
    };
    // 内置超管角色不允许删除：删除会使 admin 失去超管短路，且 seed 只补缺不重建，
    // 系统将永久失守。
    if role.role_key == SUPER_ROLE_KEY {
        return Err(AppError::Biz(
            "系统内置超级管理员角色不允许删除".to_string(),
        ));
    }
    role_repo::soft_delete_role_in_tx(txn, id).await?;
    Ok(())
}

/// 更新角色状态（启用/禁用）；内置超管角色 `super` 不允许修改状态（审计字段由 repo 盖章）。
pub async fn update_role_status(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &UpdateRoleStatusReq,
) -> Result<bool, AppError> {
    let Some(role) = role_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz("角色不存在".to_string()));
    };

    // 内置超管管理员不允许修改状态。
    if role.role_key == SUPER_ROLE_KEY {
        return Err(AppError::Biz(
            "系统内置超级管理员角色不允许修改状态".to_string(),
        ));
    }

    let model = sys_role::ActiveModel {
        id: Set(req.id),
        status: Set(req.status),
        ..Default::default()
    };
    role_repo::update_role(db, model, actor_id).await?;

    Ok(true)
}

/// 查询角色关联的菜单 ID 列表（排除软删除）；不存在返回空列表。
pub async fn get_role_menu_ids(db: &impl ConnectionTrait, id: u64) -> Result<Vec<u64>, AppError> {
    Ok(role_repo::find_menu_ids_by_role_id(db, id).await?)
}

/// 查询角色关联的 API 权限点 ID 列表（排除软删除）；不存在返回空列表。
pub async fn get_role_api_ids(db: &impl ConnectionTrait, id: u64) -> Result<Vec<u64>, AppError> {
    Ok(role_repo::find_api_ids_by_role_id(db, id).await?)
}

/// 全量角色（排除软删，含禁用；**排除内置超管**）。
///
/// 超管角色由后端短路，不应出现在「可分配角色」列表中——属业务规则，
/// 由 service 过滤；repo 只提供全量数据原语。
pub async fn get_all_roles(db: &impl ConnectionTrait) -> Result<Vec<sys_role::Model>, AppError> {
    let roles = role_repo::find_all(db).await?;
    Ok(roles
        .into_iter()
        .filter(|role| role.role_key != SUPER_ROLE_KEY)
        .collect())
}

/// 全量启用角色（排除软删与禁用；**排除内置超管**，同 `get_all_roles`）。
pub async fn get_all_enabled_roles(
    db: &impl ConnectionTrait,
) -> Result<Vec<sys_role::Model>, AppError> {
    let roles = role_repo::find_all_enabled(db).await?;
    Ok(roles
        .into_iter()
        .filter(|role| role.role_key != SUPER_ROLE_KEY)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_menu, sys_role_api, sys_role_menu};
    use crate::modules::system::role::dto::{CreateRoleReq, UpdateRoleReq};
    use crate::utils::error::AppError;
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

    /// 构造创建角色请求：默认启用、无菜单/API 关联。
    fn create_req(role_name: String, role_key: String) -> CreateRoleReq {
        CreateRoleReq {
            role_name,
            role_key,
            sort: 0,
            status: 1,
            remark: "service 层测试".to_string(),
            menu_ids: vec![],
            api_ids: vec![],
        }
    }

    async fn seed_role(
        db: &impl ConnectionTrait,
        role_name: &str,
        role_key: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(role_name.to_string()),
            role_key: Set(role_key.to_string()),
            sort: Set(0),
            status: Set(status),
            remark: Set(String::new()),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_menu(db: &impl ConnectionTrait) -> sys_menu::Model {
        sys_menu::ActiveModel {
            parent_id: Set(0),
            path: Set(format!("/{}", unique("menu_path"))),
            name: Set(unique("menu_name")),
            component: Set(format!("#/views/{}.vue", unique("menu_comp"))),
            title: Set(unique("menu_title")),
            icon: Set("mdi:test".to_string()),
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

    async fn seed_api(db: &impl ConnectionTrait) -> sys_api::Model {
        sys_api::ActiveModel {
            path: Set(format!("/api/v1/{}/list", unique("svc_api"))),
            method: Set("POST".to_string()),
            description: Set("service 层测试 API".to_string()),
            api_group: Set("role".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 创建时 role_ids 含重复 id 应在写关联前被拒。
    ///
    /// 输出端口（`role/validate.rs`）已拦重复，但 `*_in_tx` 是事务内不变量守卫的所在地，
    /// 调用方绕过 validate 时不能把 `uk(role_id, menu_id)` 冲突漏成 `internal error`。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn create_role_rejects_duplicate_menu_and_api_ids() {
        let txn = test_txn().await;
        let menu = seed_menu(&txn).await;
        let api = seed_api(&txn).await;

        let mut dup_menu = create_req(unique("dup_menu_name"), unique("dup_menu_key"));
        dup_menu.menu_ids = vec![menu.id, menu.id];
        let menu_result = create_role_in_tx(&txn, ACTOR_ID, &dup_menu).await;

        let mut dup_api = create_req(unique("dup_api_name"), unique("dup_api_key"));
        dup_api.api_ids = vec![api.id, api.id];
        let api_result = create_role_in_tx(&txn, ACTOR_ID, &dup_api).await;

        assert!(
            matches!(menu_result, Err(AppError::Biz(ref m)) if m.contains("菜单重复")),
            "重复绑定同一菜单应被业务层拒绝，实际：{menu_result:?}"
        );
        assert!(
            matches!(api_result, Err(AppError::Biz(ref m)) if m.contains("接口重复")),
            "重复绑定同一接口应被业务层拒绝，实际：{api_result:?}"
        );
    }

    /// 更新时 menu_ids / api_ids 含重复 id 同样应被拒（全量替换链路）。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_role_rejects_duplicate_menu_and_api_ids() {
        let txn = test_txn().await;
        let role = seed_role(
            &txn,
            &unique("dup_upd_role"),
            &unique("dup_upd_key"),
            1,
            None,
        )
        .await;
        let menu = seed_menu(&txn).await;
        let api = seed_api(&txn).await;

        let dup_menu = bind_menu_req(&role, vec![menu.id, menu.id]);
        let menu_result = update_role_in_tx(&txn, ACTOR_ID, &dup_menu).await;

        let mut dup_api = bind_menu_req(&role, Vec::new());
        dup_api.api_ids = vec![api.id, api.id];
        let api_result = update_role_in_tx(&txn, ACTOR_ID, &dup_api).await;

        assert!(
            matches!(menu_result, Err(AppError::Biz(ref m)) if m.contains("菜单重复")),
            "重复绑定同一菜单应被业务层拒绝，实际：{menu_result:?}"
        );
        assert!(
            matches!(api_result, Err(AppError::Biz(ref m)) if m.contains("接口重复")),
            "重复绑定同一接口应被业务层拒绝，实际：{api_result:?}"
        );
    }

    /// 硬删角色及其关联（真连接用例必须真实提交，无法用事务回滚隔离，只能手工清理）。
    async fn hard_delete_role(db: &DatabaseConnection, role_id: u64) {
        sys_role_menu::Entity::delete_many()
            .filter(sys_role_menu::Column::RoleId.eq(role_id))
            .exec(db)
            .await
            .unwrap();
        sys_role_api::Entity::delete_many()
            .filter(sys_role_api::Column::RoleId.eq(role_id))
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_many()
            .filter(sys_role::Column::Id.eq(role_id))
            .exec(db)
            .await
            .unwrap();
    }

    /// 硬删菜单及其关联（含软删记录，按 id 直删）。
    async fn hard_delete_menu(db: &DatabaseConnection, menu_id: u64) {
        sys_role_menu::Entity::delete_many()
            .filter(sys_role_menu::Column::MenuId.eq(menu_id))
            .exec(db)
            .await
            .unwrap();
        sys_menu::Entity::delete_many()
            .filter(sys_menu::Column::Id.eq(menu_id))
            .exec(db)
            .await
            .unwrap();
    }

    /// 该角色是否仍绑定指定菜单（`sys_role_menu` 硬删表，有行即绑定）。
    async fn role_binds_menu(db: &DatabaseConnection, role_id: u64, menu_id: u64) -> bool {
        sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(role_id))
            .filter(sys_role_menu::Column::MenuId.eq(menu_id))
            .one(db)
            .await
            .unwrap()
            .is_some()
    }

    /// 菜单是否已软删（`find_by_id` 会过滤软删，这里直查主键）。
    async fn menu_is_soft_deleted(db: &DatabaseConnection, menu_id: u64) -> bool {
        sys_menu::Entity::find()
            .filter(sys_menu::Column::Id.eq(menu_id))
            .one(db)
            .await
            .unwrap()
            .is_none_or(|menu| menu.deleted_at.is_some())
    }

    /// 创建时 role_key 重复（含软删除占位）应被业务层拒绝。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn create_role_rejects_duplicate_role_key_including_soft_deleted() {
        let txn = test_txn().await;
        // 数据库唯一索引不允许两条相同 role_key 共存（含软删），
        // 因此分别用两个 key 验证「正常占位」与「软删占位」都会拒绝新角色。
        let key_live = unique("dup_live");
        let key_deleted = unique("dup_deleted");
        let _live = seed_role(&txn, &unique("live_role"), &key_live, 1, None).await;
        let _deleted = seed_role(
            &txn,
            &unique("deleted_role"),
            &key_deleted,
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let result_live = create_role_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(unique("dup_live_name"), key_live.clone()),
        )
        .await;
        let result_deleted = create_role_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(unique("dup_deleted_name"), key_deleted.clone()),
        )
        .await;

        assert!(
            matches!(result_live, Err(AppError::Biz(_))),
            "role_key 被正常角色占用时 create_role 应返回 Biz 业务错误，实际：{result_live:?}"
        );
        assert!(
            matches!(result_deleted, Err(AppError::Biz(_))),
            "role_key 被软删角色占用时 create_role 应返回 Biz 业务错误，实际：{result_deleted:?}"
        );
    }

    /// 更新时 role_key 与他人重复应被拒绝，但保留自身 key 不算重复。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_role_rejects_duplicate_role_key_excluding_self() {
        let txn = test_txn().await;
        let key_a = unique("key_a");
        let key_b = unique("key_b");
        let _role_a = seed_role(&txn, &unique("role_a"), &key_a, 1, None).await;
        let role_b = seed_role(&txn, &unique("role_b"), &key_b, 1, None).await;

        let update_req = |role_name: String, role_key: String| UpdateRoleReq {
            id: role_b.id,
            role_name,
            role_key,
            sort: 0,
            status: 1,
            remark: String::new(),
            menu_ids: Vec::new(),
            api_ids: Vec::new(),
        };

        let dup = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(unique("role_b_dup"), key_a.clone()),
        )
        .await;
        let keep_self = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(unique("role_b_renamed"), key_b.clone()),
        )
        .await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 role_key 应返回 Biz 业务错误，实际：{dup:?}"
        );

        let updated = keep_self.expect("保留自身 role_key 应更新成功");
        assert_eq!(updated.role_key, key_b);
    }

    /// 更新不存在的角色（或已软删角色）应返回业务错误。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_role_returns_biz_error_when_role_missing() {
        let txn = test_txn().await;

        let missing = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: 9_999_999_999,
                role_name: "不存在".to_string(),
                role_key: unique("missing_key"),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: Vec::new(),
                api_ids: Vec::new(),
            },
        )
        .await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "更新不存在的角色应返回 Biz 业务错误，实际：{missing:?}"
        );

        let key = unique("deleted_key");
        let deleted = seed_role(
            &txn,
            "已删角色",
            &key,
            1,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let update_deleted = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: deleted.id,
                role_name: "改已删角色".to_string(),
                role_key: key.clone(),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: Vec::new(),
                api_ids: Vec::new(),
            },
        )
        .await;
        assert!(
            matches!(update_deleted, Err(AppError::Biz(_))),
            "更新已软删角色应返回 Biz 业务错误，实际：{update_deleted:?}"
        );
    }

    /// 全量更新：主表字段被覆盖，menu_ids / api_ids 全量替换（空数组即清空）。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_role_full_replaces_links() {
        let txn = test_txn().await;
        let key = unique("keep_links");
        let role_name = unique("full_role");
        let menu_a = seed_menu(&txn).await;
        let menu_b = seed_menu(&txn).await;
        let api_a = seed_api(&txn).await;
        let api_b = seed_api(&txn).await;

        let created = create_role_in_tx(
            &txn,
            ACTOR_ID,
            &CreateRoleReq {
                role_name: role_name.clone(),
                role_key: key.clone(),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: vec![menu_a.id, menu_b.id],
                api_ids: vec![api_a.id, api_b.id],
            },
        )
        .await
        .expect("创建带关联角色应成功");

        let updated = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: created.id,
                role_name: format!("{role_name}_v2"),
                role_key: key.clone(),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: vec![menu_a.id],
                api_ids: vec![api_a.id],
            },
        )
        .await
        .expect("全量更新应成功");
        assert_eq!(updated.role_name, format!("{role_name}_v2"));

        let menu_links = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        let api_links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();

        assert_eq!(menu_links.len(), 1, "全量替换后只剩新菜单关联");
        assert_eq!(menu_links[0].menu_id, menu_a.id);
        assert_eq!(api_links.len(), 1, "全量替换后只剩新 API 关联");
        assert_eq!(api_links[0].api_id, api_a.id);

        // 传空数组 = 清空关联
        update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: created.id,
                role_name: format!("{role_name}_clear"),
                role_key: key.clone(),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: Vec::new(),
                api_ids: Vec::new(),
            },
        )
        .await
        .expect("清空关联应成功");
        let menu_links_cleared = sys_role_menu::Entity::find()
            .filter(sys_role_menu::Column::RoleId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        let api_links_cleared = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();

        assert!(menu_links_cleared.is_empty(), "空数组应清空菜单关联");
        assert!(api_links_cleared.is_empty(), "空数组应清空 API 关联");
    }

    /// 已存在的 `super` 是全局唯一角色，测试只能复用；事务内新建则随回滚消失。
    async fn load_super_role(db: &impl ConnectionTrait) -> sys_role::Model {
        if let Some(role) = sys_role::Entity::find()
            .filter(sys_role::Column::RoleKey.eq(SUPER_ROLE_KEY))
            .one(db)
            .await
            .unwrap()
        {
            return role;
        }
        sys_role::ActiveModel {
            role_name: Set("超级管理员".to_string()),
            role_key: Set(SUPER_ROLE_KEY.to_string()),
            sort: Set(0),
            status: Set(1),
            remark: Set("service 层测试创建".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 内置超管角色不允许被更新：关键回归——以前校验的是 req.role_key（新值），
    /// 把 super 改名为其他 key 可绕过保留字检查，导致 admin 失去超管短路。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_role_rejects_super_role_even_with_renamed_key() {
        let txn = test_txn().await;
        let super_role = load_super_role(&txn).await;

        let rename_away = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: super_role.id,
                role_name: unique("hijack_name"),
                role_key: unique("hijack_key"),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: Vec::new(),
                api_ids: Vec::new(),
            },
        )
        .await;
        assert!(
            matches!(rename_away, Err(AppError::Biz(_))),
            "改名转移内置超管 role_key 应被拒绝: {rename_away:?}"
        );

        let keep_key = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: super_role.id,
                role_name: unique("keep_key_name"),
                role_key: SUPER_ROLE_KEY.to_string(),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: Vec::new(),
                api_ids: Vec::new(),
            },
        )
        .await;
        assert!(
            matches!(keep_key, Err(AppError::Biz(_))),
            "原键原值更新内置超管同样应被拒绝: {keep_key:?}"
        );
    }

    /// 保留字：任何普通角色不得改名为内置超管键 super。
    #[tokio::test]
    async fn update_role_rejects_reserved_super_key() {
        let txn = test_txn().await;
        let role = seed_role(&txn, &unique("normal_role"), &unique("normal_key"), 1, None).await;

        let result = update_role_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateRoleReq {
                id: role.id,
                role_name: unique("normal_role_v2"),
                role_key: SUPER_ROLE_KEY.to_string(),
                sort: 0,
                status: 1,
                remark: String::new(),
                menu_ids: Vec::new(),
                api_ids: Vec::new(),
            },
        )
        .await;
        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("保留字")),
            "普通角色改名为 super 应被拒绝: {result:?}"
        );
    }

    /// 内置超管角色不允许删除：删除会使 admin 失去超管短路且 seed 无法重建。
    #[tokio::test]
    async fn delete_role_rejects_super_role() {
        let txn = test_txn().await;
        let super_role = load_super_role(&txn).await;

        let result = delete_role_in_tx(&txn, super_role.id).await;
        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("不允许删除")),
            "删除内置超管角色应被拒绝: {result:?}"
        );

        let still_alive = sys_role::Entity::find()
            .filter(sys_role::Column::Id.eq(super_role.id))
            .one(&txn)
            .await
            .unwrap()
            .expect("super 角色应仍然存在");
        assert!(still_alive.deleted_at.is_none(), "super 角色不应被软删除");
    }

    /// 可分配角色列表必须排除内置超管（该业务规则在 service 层，repo 只给全量）。
    #[tokio::test]
    async fn get_all_roles_and_enabled_exclude_super_role() {
        let txn = test_txn().await;
        let super_role = load_super_role(&txn).await;
        let normal = seed_role(&txn, &unique("list_role"), &unique("list_key"), 1, None).await;

        let all = get_all_roles(&txn).await.unwrap();
        assert!(
            all.iter().any(|role| role.id == normal.id),
            "普通角色应出现在全量列表中"
        );
        assert!(
            all.iter().all(|role| role.id != super_role.id),
            "内置超管不应出现在可分配角色列表"
        );

        let enabled = get_all_enabled_roles(&txn).await.unwrap();
        assert!(
            enabled.iter().any(|role| role.id == normal.id),
            "启用角色应出现在启用列表中"
        );
        assert!(
            enabled.iter().all(|role| role.id != super_role.id),
            "内置超管不应出现在启用角色列表"
        );
    }

    /// 绑定请求夹具：沿用角色原有键名，只替换菜单绑定。
    fn bind_menu_req(role: &sys_role::Model, menu_ids: Vec<u64>) -> UpdateRoleReq {
        UpdateRoleReq {
            id: role.id,
            role_name: role.role_name.clone(),
            role_key: role.role_key.clone(),
            sort: 0,
            status: 1,
            remark: String::new(),
            menu_ids,
            api_ids: Vec::new(),
        }
    }

    /// 绑定前必须校验引用有效：不存在的菜单 / 接口 id 应被拒绝，而不是写进关系表。
    #[tokio::test]
    async fn role_binding_rejects_missing_menu_and_api() {
        let txn = test_txn().await;
        let role = seed_role(&txn, &unique("miss_role"), &unique("miss_key"), 1, None).await;

        let mut create = create_req(unique("miss_name"), unique("miss_key2"));
        create.menu_ids = vec![9_999_999_999];
        let create_result = create_role_in_tx(&txn, ACTOR_ID, &create).await;

        let mut update = bind_menu_req(&role, Vec::new());
        update.api_ids = vec![9_999_999_998];
        let update_result = update_role_in_tx(&txn, ACTOR_ID, &update).await;

        assert!(
            matches!(create_result, Err(AppError::Biz(ref m)) if m.contains("菜单不存在")),
            "绑定不存在的菜单应被拒绝: {create_result:?}"
        );
        assert!(
            matches!(update_result, Err(AppError::Biz(ref m)) if m.contains("接口不存在")),
            "绑定不存在的接口应被拒绝: {update_result:?}"
        );
    }

    /// 并发「删除菜单」+「角色绑定该菜单」：绑定方必须读到删除结果并拒绝，否则留下悬挂绑定。
    ///
    /// 确定性交错：删除方先发起并**持有菜单行锁**（未提交），绑定方随后发起 → 卡在行锁上 →
    /// 删除方提交后绑定方读到「菜单已软删」→ 报「菜单不存在」，关系表不产生该行。
    /// 两连接必须真实提交才能互相看见，故用真连接 + 手工清理。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn update_role_rejects_menu_deleted_concurrently() {
        let db = test_db().await;
        let role = seed_role(&db, &unique("ct_role"), &unique("ct_key"), 1, None).await;
        let menu = seed_menu(&db).await;

        // 删除方：软删菜单 + 级联清理关联，均未提交（菜单行锁仍在手里）
        let txn_delete = db.begin().await.unwrap();
        menu_service::delete_menu_in_tx(&txn_delete, menu.id)
            .await
            .expect("前置：删菜单应放行");

        // 绑定方：另一条连接上发起角色更新，卡在删除方持有的菜单行锁上
        let db_bind = db.clone();
        let role_req = bind_menu_req(&role, vec![menu.id]);
        let handle = tokio::spawn(async move { update_role(&db_bind, ACTOR_ID, &role_req).await });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        txn_delete.commit().await.unwrap();

        let result = handle.await.unwrap();
        let dangling = role_binds_menu(&db, role.id, menu.id).await;
        let menu_deleted = menu_is_soft_deleted(&db, menu.id).await;
        hard_delete_role(&db, role.id).await;
        hard_delete_menu(&db, menu.id).await;

        assert!(menu_deleted, "前置：菜单应已被软删");
        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("菜单不存在")),
            "绑定已软删的菜单应被拒绝: {result:?}"
        );
        assert!(!dangling, "不应留下指向已软删菜单的悬挂绑定");
    }

    /// 真并发冒烟：删菜单 与 角色绑定该菜单 同时发起，不得留下悬挂绑定。
    ///
    /// 两种合法结果：绑定先赢（随后删除的级联清理会删掉该绑定）或删除先赢（绑定被拒）。
    /// 只断言与调度无关的不变量：不存在「菜单已软删 + 关系表仍绑着它」。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_delete_menu_and_bind_role_leaves_no_dangling_link() {
        let db = test_db().await;
        let role = seed_role(&db, &unique("cc_role"), &unique("cc_key"), 1, None).await;
        let menu = seed_menu(&db).await;

        let (db_delete, db_bind) = (db.clone(), db.clone());
        let (role_id, menu_id) = (role.id, menu.id);
        let role_req = bind_menu_req(&role, vec![menu.id]);
        let delete_handle =
            tokio::spawn(async move { menu_service::delete_menu(&db_delete, menu_id).await });
        let bind_handle =
            tokio::spawn(async move { update_role(&db_bind, ACTOR_ID, &role_req).await });
        let (deleted, bound) = (delete_handle.await.unwrap(), bind_handle.await.unwrap());

        let dangling = role_binds_menu(&db, role_id, menu_id).await;
        let menu_deleted = menu_is_soft_deleted(&db, menu_id).await;
        hard_delete_role(&db, role_id).await;
        hard_delete_menu(&db, menu_id).await;

        assert!(
            !(dangling && menu_deleted),
            "不应留下悬挂绑定: deleted={deleted:?} bound={bound:?}"
        );
    }
}
