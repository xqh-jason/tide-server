use crate::modules::permission::SUPER_ROLE_KEY;
use crate::modules::role::dto::UpdateRoleStatusReq;
use crate::utils::PageData;
use crate::{
    modules::role::{
        dto::{CreateRoleReq, RoleFilter, RoleListReq, UpdateRoleReq},
        repo as role_repo,
    },
    utils::error::AppError,
};
use sea_orm::{
    ActiveValue::Set, ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait,
};

use crate::entity::sys_role;

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

/// 全量角色（排除软删，含禁用）。
pub async fn get_all_roles(db: &impl ConnectionTrait) -> Result<Vec<sys_role::Model>, AppError> {
    Ok(role_repo::find_all(db).await?)
}

/// 全量启用角色（排除软删与禁用）。
pub async fn get_all_enabled_roles(
    db: &impl ConnectionTrait,
) -> Result<Vec<sys_role::Model>, AppError> {
    Ok(role_repo::find_all_enabled(db).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_menu, sys_role_api, sys_role_menu};
    use crate::modules::role::dto::{CreateRoleReq, UpdateRoleReq};
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
}
