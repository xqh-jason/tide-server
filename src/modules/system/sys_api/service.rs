//! API 权限点业务规则。

use sea_orm::{
    ActiveValue::Set, ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait,
};

use crate::modules::system::role::service as role_service;
use crate::modules::system::sys_api::dto::{ApiFilter, ApiListReq, UpdateApiReq};
use crate::modules::system::sys_api::repo as api_repo;
use crate::utils::PageData;
use crate::{entity::sys_api, modules::system::sys_api::dto::CreateApiReq, utils::error::AppError};

/// 分页查询 API（keyword 匹配 path/description/api_group，status/method 精确，审计过滤）。
pub async fn page_apis(
    db: &impl ConnectionTrait,
    req: &ApiListReq,
) -> Result<PageData<sys_api::Model>, AppError> {
    let model = api_repo::find_page(
        db,
        &ApiFilter {
            method: req.method.clone(),
            status: req.status,
            keyword: req.keyword.clone(),
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

/// 对外入口：开事务后委托 `create_api_in_tx`，成功后提交。
pub async fn create_api(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreateApiReq,
) -> Result<sys_api::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_api_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（path+method 查重 + 写入），不管理事务边界。
/// 供对外入口与测试外层事务调用。
pub(crate) async fn create_api_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreateApiReq,
) -> Result<sys_api::Model, AppError> {
    // path + method 查重（含软删占位）
    if let Some(existing) =
        api_repo::find_by_path_method_include_deleted(txn, &req.path, &req.method).await?
    {
        return Err(AppError::Biz(format!(
            "API 路径与方法已存在：{} {}",
            existing.path, existing.method
        )));
    }

    // 角色绑定查重
    role_service::ensure_roles_exist(txn, &req.role_ids).await?;

    let model = sys_api::ActiveModel {
        path: Set(req.path.clone()),
        method: Set(req.method.clone()),
        description: Set(req.description.clone()),
        api_group: Set(req.api_group.clone()),
        status: Set(req.status),
        ..Default::default()
    };
    let model = api_repo::create_api_in_tx(txn, model, req.role_ids.clone(), actor_id).await?;
    Ok(model)
}

/// 对外入口：开事务后委托 `update_api_in_tx`，成功后提交。
pub async fn update_api(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateApiReq,
) -> Result<sys_api::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;

    let result = update_api_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（存在性 + path/method 查重排除自身 + 写入），不管理事务边界。
/// 供对外入口与测试外层事务调用。
pub(crate) async fn update_api_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateApiReq,
) -> Result<sys_api::Model, AppError> {
    // 角色绑定查重
    role_service::ensure_roles_exist(txn, &req.role_ids).await?;

    // path + method 查重（含软删），排除自身
    let dup =
        api_repo::find_by_path_method_include_deleted(txn, req.path.as_str(), req.method.as_str())
            .await?
            .is_some_and(|existing| existing.id != req.id);
    if dup {
        return Err(AppError::Biz("API 路径与方法已存在".to_string()));
    }

    // 检查 API 是否存在
    let Some(_) = api_repo::find_by_id(txn, req.id).await? else {
        return Err(AppError::Biz(format!("API 不存在：{}", req.id)));
    };

    let model = sys_api::ActiveModel {
        id: Set(req.id),
        path: Set(req.path.clone()),
        method: Set(req.method.clone()),
        description: Set(req.description.clone()),
        api_group: Set(req.api_group.clone()),
        status: Set(req.status),
        ..Default::default()
    };
    let model = api_repo::update_api_in_tx(txn, model, req.role_ids.clone(), actor_id).await?;
    Ok(model)
}

/// 查询单个 API 详情（排除软删除）；不存在返回业务错误。
pub async fn get_api(db: &impl ConnectionTrait, id: u64) -> Result<sys_api::Model, AppError> {
    let Some(model) = api_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("API 不存在：{id}")));
    };
    Ok(model)
}

/// 对外入口：开事务后委托 `delete_api_in_tx`，成功后提交。
pub async fn delete_api(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_api_in_tx(&txn, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 事务内完整业务（存在性 + 软删），不管理事务边界。
/// 供对外入口与测试外层事务调用。
pub(crate) async fn delete_api_in_tx(txn: &DatabaseTransaction, id: u64) -> Result<(), AppError> {
    let Some(_) = api_repo::find_by_id(txn, id).await? else {
        return Err(AppError::Biz(format!("API 不存在：{id}")));
    };
    api_repo::soft_delete_api_in_tx(txn, id).await?;
    Ok(())
}

/// 查询所有 API（排除软删除）。
pub async fn get_all_apis(db: &impl ConnectionTrait) -> Result<Vec<sys_api::Model>, AppError> {
    Ok(api_repo::find_all(db).await?)
}

/// 批量按 id 查有效 API 权限并加锁（跨域引用校验用），空入参返回空数组。
///
/// 与普通读的区别是**加了排他锁**：跨域写入（role 域写 `sys_role_api`）必须先锁住被引用的
/// API 行，才能与「删除 API」的关联清理串行化——否则删除方清完关联后绑定方才插入，会留下
/// 指向已软删接口的悬挂绑定。须在事务内调用。
pub async fn find_by_ids_for_update(
    txn: &DatabaseTransaction,
    ids: &[u64],
) -> Result<Vec<sys_api::Model>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(api_repo::find_by_ids_for_update(txn, ids).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_role, sys_role_api};
    use crate::modules::system::sys_api::dto::{CreateApiReq, UpdateApiReq};
    use crate::utils::error::AppError;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, PaginatorTrait,
        QueryFilter, Set,
    };
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

    fn create_req(path: String, method: String, role_ids: Vec<u64>) -> CreateApiReq {
        CreateApiReq {
            path,
            method,
            description: unique("desc"),
            api_group: "service_test".to_string(),
            status: 1,
            role_ids,
        }
    }

    async fn seed_api(db: &impl ConnectionTrait, path: &str, method: &str) -> sys_api::Model {
        sys_api::ActiveModel {
            path: Set(path.to_string()),
            method: Set(method.to_string()),
            description: Set(unique("seed_desc")),
            api_group: Set("service_test".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(db: &impl ConnectionTrait) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("api_svc_role")),
            role_key: Set(unique("api_svc_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 带软删标记的角色：`sys_role.deleted_at` 直写，主表查询会把它当作不存在。
    async fn seed_soft_deleted_role(db: &impl ConnectionTrait) -> sys_role::Model {
        let role = seed_role(db).await;
        let mut model: sys_role::ActiveModel = role.clone().into();
        model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        model.update(db).await.unwrap()
    }

    fn update_req(id: u64, path: String, role_ids: Vec<u64>) -> UpdateApiReq {
        UpdateApiReq {
            id,
            path,
            method: "POST".to_string(),
            description: unique("update_desc"),
            api_group: "service_test".to_string(),
            status: 1,
            role_ids,
        }
    }

    /// 统计某 API 的授权关联行数（`sys_role_api` 硬删表，有行即授权）。
    async fn link_count(db: &impl ConnectionTrait, api_id: u64) -> u64 {
        sys_role_api::Entity::find()
            .filter(sys_role_api::Column::ApiId.eq(api_id))
            .count(db)
            .await
            .unwrap()
    }

    /// 创建时 path + method 重复（含软删占位）应被业务层拒绝。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn create_api_rejects_duplicate_path_method_including_soft_deleted() {
        let txn = test_txn().await;
        let path_live = format!("/api/v1/{}/dup_live", unique("create"));
        let path_deleted = format!("/api/v1/{}/dup_deleted", unique("create"));
        let _live = seed_api(&txn, &path_live, "POST").await;
        let deleted = seed_api(&txn, &path_deleted, "POST").await;
        let mut deleted_model: sys_api::ActiveModel = deleted.clone().into();
        deleted_model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        deleted_model.update(&txn).await.unwrap();

        let result_live = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path_live.clone(), "POST".to_string(), vec![]),
        )
        .await;
        let result_deleted = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path_deleted.clone(), "POST".to_string(), vec![]),
        )
        .await;

        assert!(
            matches!(result_live, Err(AppError::Biz(_))),
            "正常 API 占用的 path+method 应被拒绝，实际：{result_live:?}"
        );
        assert!(
            matches!(result_deleted, Err(AppError::Biz(_))),
            "软删 API 占用的 path+method 应被拒绝，实际：{result_deleted:?}"
        );
    }

    /// 更新时 path + method 与他人重复应被拒绝，但保留自身组合不算重复。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_api_rejects_duplicate_path_method_excluding_self() {
        let txn = test_txn().await;
        let path_a = format!("/api/v1/{}/a", unique("update"));
        let path_b = format!("/api/v1/{}/b", unique("update"));
        let _api_a = seed_api(&txn, &path_a, "POST").await;
        let api_b = seed_api(&txn, &path_b, "POST").await;

        let dup = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateApiReq {
                id: api_b.id,
                path: path_a.clone(),
                method: "POST".to_string(),
                description: "更新测试".to_string(),
                api_group: "service_test".to_string(),
                status: 1,
                role_ids: vec![],
            },
        )
        .await;
        let keep_self = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateApiReq {
                id: api_b.id,
                path: path_b.clone(),
                method: "POST".to_string(),
                description: "保留自身".to_string(),
                api_group: "service_test".to_string(),
                status: 1,
                role_ids: vec![],
            },
        )
        .await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 path+method 应返回 Biz 业务错误，实际：{dup:?}"
        );
        let updated = keep_self.expect("保留自身 path+method 应更新成功");
        assert_eq!(updated.path, path_b);
    }

    /// 更新不存在的 API（或已软删 API）应返回业务错误。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_api_returns_biz_error_when_api_missing() {
        let txn = test_txn().await;

        let missing = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateApiReq {
                id: 9_999_999_999,
                path: "/missing".to_string(),
                method: "POST".to_string(),
                description: String::new(),
                api_group: String::new(),
                status: 1,
                role_ids: vec![],
            },
        )
        .await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "更新不存在的 API 应返回 Biz 业务错误，实际：{missing:?}"
        );

        let path = format!("/api/v1/{}/deleted", unique("update"));
        let deleted = seed_api(&txn, &path, "POST").await;
        let mut deleted_model: sys_api::ActiveModel = deleted.clone().into();
        deleted_model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        deleted_model.update(&txn).await.unwrap();

        let update_deleted = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateApiReq {
                id: deleted.id,
                path: path.clone(),
                method: "POST".to_string(),
                description: String::new(),
                api_group: String::new(),
                status: 1,
                role_ids: vec![],
            },
        )
        .await;
        assert!(
            matches!(update_deleted, Err(AppError::Biz(_))),
            "更新已软删 API 应返回 Biz 业务错误，实际：{update_deleted:?}"
        );
    }

    /// 更新时 role_ids 全量替换授权关联（空数组即清空）。
    #[tokio::test]
    // 业务入口拆为 *_in_tx：被测逻辑不自行 begin/commit，测试在外层事务中执行，
    // 断言失败/panic 由事务 Drop 自动回滚，无需手写清理。
    async fn update_api_full_replaces_role_links() {
        let txn = test_txn().await;
        let role_old = seed_role(&txn).await;
        let role_new = seed_role(&txn).await;
        let path = format!("/api/v1/{}/links", unique("update"));

        let created = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path.clone(), "POST".to_string(), vec![role_old.id]),
        )
        .await
        .expect("创建带授权 API 应成功");
        let updated = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &UpdateApiReq {
                id: created.id,
                path: path.clone(),
                method: "POST".to_string(),
                description: "替换授权".to_string(),
                api_group: "service_test".to_string(),
                status: 1,
                role_ids: vec![role_new.id],
            },
        )
        .await
        .expect("全量替换授权应成功");

        let links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::ApiId.eq(created.id))
            .all(&txn)
            .await
            .unwrap();
        assert_eq!(updated.description, "替换授权");
        assert_eq!(links.len(), 1, "旧授权应被清空，只剩新授权");
        assert_eq!(links[0].role_id, role_new.id);
    }

    /// 授权角色 id 重复时应在写关联前被拒（DB 侧 `uk(role_id, api_id)` 会撞成 Internal）。
    #[tokio::test]
    async fn create_api_rejects_duplicate_role_ids() {
        let txn = test_txn().await;
        let role = seed_role(&txn).await;
        let path = format!("/api/v1/{}/dup_role", unique("create"));

        let result = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path.clone(), "POST".to_string(), vec![role.id, role.id]),
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("角色重复")),
            "重复授权同一角色应被业务层拒绝，实际：{result:?}"
        );
    }

    /// 更新时授权角色 id 重复同样应被拒（此时全量替换关联已删旧行，若不拦则写入必撞主键）。
    #[tokio::test]
    async fn update_api_rejects_duplicate_role_ids() {
        let txn = test_txn().await;
        let role = seed_role(&txn).await;
        let path = format!("/api/v1/{}/upd_dup_role", unique("update"));
        let created = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path.clone(), "POST".to_string(), vec![role.id]),
        )
        .await
        .expect("前置：创建带授权 API 应成功");

        let result = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(created.id, path, vec![role.id, role.id]),
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("角色重复")),
            "重复授权同一角色应被业务层拒绝，实际：{result:?}"
        );
    }

    /// 授权给不存在的角色应被拒，且不留下指向该角色的孤儿关联行。
    #[tokio::test]
    async fn create_api_rejects_missing_role() {
        let txn = test_txn().await;
        let missing_role_id = 9_999_999_997;
        let path = format!("/api/v1/{}/miss_role", unique("create"));

        let result = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path, "POST".to_string(), vec![missing_role_id]),
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("角色不存在")),
            "授权不存在的角色应被业务层拒绝，实际：{result:?}"
        );
        let orphans = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(missing_role_id))
            .count(&txn)
            .await
            .unwrap();
        assert_eq!(orphans, 0, "拒绝后不应留下孤儿授权行");
    }

    /// 更新时授权给不存在的角色应被拒，且不留下孤儿关联行。
    #[tokio::test]
    async fn update_api_rejects_missing_role() {
        let txn = test_txn().await;
        let path = format!("/api/v1/{}/upd_miss_role", unique("update"));
        let created = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path.clone(), "POST".to_string(), vec![]),
        )
        .await
        .expect("前置：创建无授权 API 应成功");
        let missing_role_id = 9_999_999_996;

        let result = update_api_in_tx(
            &txn,
            ACTOR_ID,
            &update_req(created.id, path, vec![missing_role_id]),
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("角色不存在")),
            "授权不存在的角色应被业务层拒绝，实际：{result:?}"
        );
        assert_eq!(
            link_count(&txn, created.id).await,
            0,
            "拒绝后不应留下孤儿授权行"
        );
    }

    /// 已软删角色视为不存在：授权应被拒（与菜单 / 接口侧的软删语义一致）。
    #[tokio::test]
    async fn create_api_rejects_soft_deleted_role() {
        let txn = test_txn().await;
        let role = seed_soft_deleted_role(&txn).await;
        let path = format!("/api/v1/{}/soft_role", unique("create"));

        let result = create_api_in_tx(
            &txn,
            ACTOR_ID,
            &create_req(path, "POST".to_string(), vec![role.id]),
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("角色不存在")),
            "授权已软删角色应被业务层拒绝，实际：{result:?}"
        );
        assert_eq!(
            link_count(&txn, result.map(|api| api.id).unwrap_or(0)).await,
            0,
            "拒绝后不应留下孤儿授权行"
        );
    }
}
