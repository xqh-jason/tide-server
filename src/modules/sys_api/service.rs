//! API 权限点业务规则。

use sea_orm::{ActiveValue::Set, DatabaseConnection};

use crate::modules::sys_api::dto::{ApiFilter, ApiListReq, UpdateApiReq};
use crate::modules::sys_api::repo as api_repo;
use crate::utils::PageData;
use crate::{entity::sys_api, modules::sys_api::dto::CreateApiReq, utils::error::AppError};

/// 分页查询 API（keyword 匹配 path/description/api_group，status/method 精确）。
pub async fn page_apis(
    db: &DatabaseConnection,
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
            )?,
            created_at_end: crate::utils::datetime::parse_datetime(
                "createdAtEnd",
                &req.created_at_end,
            )?,
            updated_at_begin: crate::utils::datetime::parse_datetime(
                "updatedAtBegin",
                &req.updated_at_begin,
            )?,
            updated_at_end: crate::utils::datetime::parse_datetime(
                "updatedAtEnd",
                &req.updated_at_end,
            )?,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await?;
    Ok(model)
}

/// 创建 API：path + method 查重（含软删占位）→ 写入主表并维护角色授权关联（审计字段由 repo 盖章）。
pub async fn create_api(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreateApiReq,
) -> Result<sys_api::Model, AppError> {
    // path + method 查重（含软删占位）
    if let Some(existing) =
        api_repo::find_by_path_method_include_deleted(db, &req.path, &req.method).await?
    {
        return Err(AppError::Biz(format!(
            "API 路径与方法已存在：{} {}",
            existing.path, existing.method
        )));
    }

    let model = sys_api::ActiveModel {
        path: Set(req.path.clone()),
        method: Set(req.method.clone()),
        description: Set(req.description.clone().unwrap_or("".to_string())),
        api_group: Set(req.api_group.clone().unwrap_or("".to_string())),
        status: Set(req.status.unwrap_or(1)),
        ..Default::default()
    };
    let model = api_repo::create_api_with_links(db, model, req.role_ids.clone(), actor_id).await?;
    Ok(model)
}

/// 更新 API：判存在 → path + method 查重排除自身 → 全量覆盖并重建角色授权（审计字段由 repo 盖章）。
pub async fn update_api(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateApiReq,
) -> Result<sys_api::Model, AppError> {
    // 检查 API 是否存在
    let Some(_) = api_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz(format!("API 不存在：{}", req.id)));
    };
    // path + method 查重（含软删），排除自身
    if let Some(existing) =
        api_repo::find_by_path_method_include_deleted(db, req.path.as_str(), req.method.as_str())
            .await?
    {
        if existing.id != req.id {
            return Err(AppError::Biz("API 路径与方法已存在".to_string()));
        }
    }

    let model = sys_api::ActiveModel {
        id: Set(req.id),
        path: Set(req.path.clone()),
        method: Set(req.method.clone()),
        description: Set(req.description.clone()),
        api_group: Set(req.api_group.clone()),
        status: Set(req.status),
        ..Default::default()
    };
    let model = api_repo::update_api_with_links(db, model, req.role_ids.clone(), actor_id).await?;
    Ok(model)
}

/// 查询单个 API 详情（排除软删除）；不存在返回业务错误。
pub async fn get_api(db: &DatabaseConnection, id: u64) -> Result<sys_api::Model, AppError> {
    let Some(model) = api_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("API 不存在：{id}")));
    };
    Ok(model)
}

/// 删除 API：判存在后级联清空角色授权关联并软删主表。
pub async fn delete_api(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let Some(_) = api_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("API 不存在：{id}")));
    };
    api_repo::soft_delete_api(db, id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_role, sys_role_api};
    use crate::modules::sys_api::dto::{CreateApiReq, UpdateApiReq};
    use crate::utils::error::AppError;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
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

    fn create_req(path: String, method: String, role_ids: Vec<u64>) -> CreateApiReq {
        CreateApiReq {
            path,
            method,
            description: Some(unique("desc")),
            api_group: Some("service_test".to_string()),
            status: Some(1),
            role_ids,
        }
    }

    async fn seed_api(db: &DatabaseConnection, path: &str, method: &str) -> sys_api::Model {
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

    async fn seed_role(db: &DatabaseConnection) -> sys_role::Model {
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

    /// 清理顺序：先删关联表，再删主表。
    async fn cleanup(db: &DatabaseConnection, api_ids: &[u64], role_ids: &[u64]) {
        for api_id in api_ids {
            sys_role_api::Entity::delete_many()
                .filter(sys_role_api::Column::ApiId.eq(*api_id))
                .exec(db)
                .await
                .unwrap();
        }
        sys_api::Entity::delete_many()
            .filter(sys_api::Column::Id.is_in(api_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
        sys_role::Entity::delete_many()
            .filter(sys_role::Column::Id.is_in(role_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    /// 创建时 path + method 重复（含软删占位）应被业务层拒绝。
    #[tokio::test]
    async fn create_api_rejects_duplicate_path_method_including_soft_deleted() {
        let db = test_db().await;
        let path_live = format!("/api/v1/{}/dup_live", unique("create"));
        let path_deleted = format!("/api/v1/{}/dup_deleted", unique("create"));
        let live = seed_api(&db, &path_live, "POST").await;
        let deleted = seed_api(&db, &path_deleted, "POST").await;
        let mut deleted_model: sys_api::ActiveModel = deleted.clone().into();
        deleted_model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        deleted_model.update(&db).await.unwrap();

        let result_live = create_api(
            &db,
            ACTOR_ID,
            &create_req(path_live.clone(), "POST".to_string(), vec![]),
        )
        .await;
        let result_deleted = create_api(
            &db,
            ACTOR_ID,
            &create_req(path_deleted.clone(), "POST".to_string(), vec![]),
        )
        .await;

        cleanup(&db, &[live.id, deleted.id], &[]).await;

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
    async fn update_api_rejects_duplicate_path_method_excluding_self() {
        let db = test_db().await;
        let path_a = format!("/api/v1/{}/a", unique("update"));
        let path_b = format!("/api/v1/{}/b", unique("update"));
        let api_a = seed_api(&db, &path_a, "POST").await;
        let api_b = seed_api(&db, &path_b, "POST").await;

        let dup = update_api(
            &db,
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
        let keep_self = update_api(
            &db,
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

        cleanup(&db, &[api_a.id, api_b.id], &[]).await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 path+method 应返回 Biz 业务错误，实际：{dup:?}"
        );
        let updated = keep_self.expect("保留自身 path+method 应更新成功");
        assert_eq!(updated.path, path_b);
    }

    /// 更新不存在的 API（或已软删 API）应返回业务错误。
    #[tokio::test]
    async fn update_api_returns_biz_error_when_api_missing() {
        let db = test_db().await;

        let missing = update_api(
            &db,
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
        let deleted = seed_api(&db, &path, "POST").await;
        let mut deleted_model: sys_api::ActiveModel = deleted.clone().into();
        deleted_model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        deleted_model.update(&db).await.unwrap();

        let update_deleted = update_api(
            &db,
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
        cleanup(&db, &[deleted.id], &[]).await;
        assert!(
            matches!(update_deleted, Err(AppError::Biz(_))),
            "更新已软删 API 应返回 Biz 业务错误，实际：{update_deleted:?}"
        );
    }

    /// 更新时 role_ids 全量替换授权关联（空数组即清空）。
    #[tokio::test]
    async fn update_api_full_replaces_role_links() {
        let db = test_db().await;
        let role_old = seed_role(&db).await;
        let role_new = seed_role(&db).await;
        let path = format!("/api/v1/{}/links", unique("update"));

        let created = create_api(
            &db,
            ACTOR_ID,
            &create_req(path.clone(), "POST".to_string(), vec![role_old.id]),
        )
        .await
        .expect("创建带授权 API 应成功");
        let updated = update_api(
            &db,
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
            .all(&db)
            .await
            .unwrap();
        cleanup(&db, &[created.id], &[role_old.id, role_new.id]).await;

        assert_eq!(updated.description, "替换授权");
        assert_eq!(links.len(), 1, "旧授权应被清空，只剩新授权");
        assert_eq!(links[0].role_id, role_new.id);
    }
}
