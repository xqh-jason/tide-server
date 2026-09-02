use crate::utils::PageData;
use crate::{
    modules::role::{
        dto::{CreateRoleReq, RoleFilter, RoleListReq, UpdateRoleReq},
        repo as role_repo,
    },
    utils::error::AppError,
};
use sea_orm::{ActiveValue::Set, DatabaseConnection};

use crate::entity::sys_role;

/// 分页查询角色（keyword 模糊匹配 role_name / role_key，status 精确），排除软删除。
pub async fn page_roles(
    db: &DatabaseConnection,
    req: &RoleListReq,
) -> anyhow::Result<PageData<sys_role::Model>> {
    role_repo::find_page(
        db,
        &RoleFilter {
            keyword: req.keyword.clone(),
            status: req.status,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 批量按 id 查询有效角色（排除软删除），供权限模块等按 id 集合取角色。
pub async fn find_by_ids(
    db: &DatabaseConnection,
    ids: Vec<u64>,
) -> anyhow::Result<Vec<sys_role::Model>> {
    let roles = role_repo::find_by_ids(db, ids).await?;
    Ok(roles)
}

/// 创建角色
pub async fn create_role(
    db: &DatabaseConnection,
    req: &CreateRoleReq,
) -> Result<sys_role::Model, AppError> {
    // 检查角色键是否已存在
    let role = role_repo::find_by_role_key_include_deleted(db, &req.role_key).await?;
    if let Some(_) = role {
        return Err(AppError::Biz("角色键已存在".to_string()));
    }
    // 检查角色名称是否已存在
    let role = role_repo::find_by_role_name_include_deleted(db, &req.role_name).await?;
    if let Some(_) = role {
        return Err(AppError::Biz("角色名称已存在".to_string()));
    }

    let model = sys_role::ActiveModel {
        role_name: Set(req.role_name.clone()),
        role_key: Set(req.role_key.clone()),
        sort: Set(req.sort.unwrap_or(0)),
        status: Set(req.status.unwrap_or(1)),
        remark: Set(req.remark.clone().unwrap_or_default()),
        ..Default::default()
    };
    let menu_ids = req.menu_ids.clone().unwrap_or_default();
    let api_ids = req.api_ids.clone().unwrap_or_default();

    let model = role_repo::create_role_with_links(db, model, menu_ids, api_ids).await?;
    Ok(model)
}

/// 查询单个角色详情（排除软删除）；不存在返回业务错误。
pub async fn get_role(db: &DatabaseConnection, id: u64) -> Result<sys_role::Model, AppError> {
    let Some(role) = role_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("角色不存在：{id}")));
    };
    Ok(role)
}

/// 删除角色：判存在后软删并物理清空菜单/API 关联。
pub async fn delete_role(db: &DatabaseConnection, id: u64) -> Result<(), AppError> {
    let Some(_) = role_repo::find_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("角色不存在：{id}")));
    };
    role_repo::soft_delete_role(db, id).await?;
    Ok(())
}

/// 更新角色：判存在后键查重（排除自身），事务内全量重建菜单 / API 关联。
pub async fn update_role(
    db: &DatabaseConnection,
    req: &UpdateRoleReq,
) -> Result<sys_role::Model, AppError> {
    let Some(_) = role_repo::find_by_id(db, req.id).await? else {
        return Err(AppError::Biz("角色不存在".to_string()));
    };

    // 检查角色键是否已存在
    let role = role_repo::find_by_role_key_include_deleted(db, req.role_key.as_str()).await?;
    if let Some(role) = role {
        // 角色键已存在，且不是当前角色键
        if role.id != req.id {
            return Err(AppError::Biz("角色键已存在".to_string()));
        }
    }
    // 检查角色名称是否已存在
    let role = role_repo::find_by_role_name_include_deleted(db, req.role_name.as_str()).await?;
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

    let model =
        role_repo::update_role_with_links(db, model, req.menu_ids.clone(), req.api_ids.clone())
            .await?;
    Ok(model)
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

    /// 构造创建角色请求：默认启用、无菜单/API 关联。
    fn create_req(role_name: String, role_key: String) -> CreateRoleReq {
        CreateRoleReq {
            role_name,
            role_key,
            sort: Some(0),
            status: Some(1),
            remark: Some("service 层测试".to_string()),
            menu_ids: None,
            api_ids: None,
        }
    }

    async fn seed_role(
        db: &DatabaseConnection,
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

    async fn seed_menu(db: &DatabaseConnection) -> sys_menu::Model {
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

    async fn seed_api(db: &DatabaseConnection) -> sys_api::Model {
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

    /// 清理顺序：先删关联表，再删角色/菜单/API 主表（关系表硬删除）。
    async fn cleanup(db: &DatabaseConnection, role_ids: &[u64], menu_ids: &[u64], api_ids: &[u64]) {
        for role_id in role_ids {
            sys_role_menu::Entity::delete_many()
                .filter(sys_role_menu::Column::RoleId.eq(*role_id))
                .exec(db)
                .await
                .unwrap();
            sys_role_api::Entity::delete_many()
                .filter(sys_role_api::Column::RoleId.eq(*role_id))
                .exec(db)
                .await
                .unwrap();
        }
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
        sys_api::Entity::delete_many()
            .filter(sys_api::Column::Id.is_in(api_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    /// 创建时 role_key 重复（含软删除占位）应被业务层拒绝。
    #[tokio::test]
    async fn create_role_rejects_duplicate_role_key_including_soft_deleted() {
        let db = test_db().await;
        // 数据库唯一索引不允许两条相同 role_key 共存（含软删），
        // 因此分别用两个 key 验证「正常占位」与「软删占位」都会拒绝新角色。
        let key_live = unique("dup_live");
        let key_deleted = unique("dup_deleted");
        let live = seed_role(&db, &unique("live_role"), &key_live, 1, None).await;
        let deleted = seed_role(
            &db,
            &unique("deleted_role"),
            &key_deleted,
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;

        let result_live =
            create_role(&db, &create_req(unique("dup_live_name"), key_live.clone())).await;
        let result_deleted = create_role(
            &db,
            &create_req(unique("dup_deleted_name"), key_deleted.clone()),
        )
        .await;

        cleanup(&db, &[live.id, deleted.id], &[], &[]).await;

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
    async fn update_role_rejects_duplicate_role_key_excluding_self() {
        let db = test_db().await;
        let key_a = unique("key_a");
        let key_b = unique("key_b");
        let role_a = seed_role(&db, &unique("role_a"), &key_a, 1, None).await;
        let role_b = seed_role(&db, &unique("role_b"), &key_b, 1, None).await;

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

        let dup = update_role(&db, &update_req(unique("role_b_dup"), key_a.clone())).await;
        let keep_self =
            update_role(&db, &update_req(unique("role_b_renamed"), key_b.clone())).await;

        cleanup(&db, &[role_a.id, role_b.id], &[], &[]).await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 role_key 应返回 Biz 业务错误，实际：{dup:?}"
        );

        let updated = keep_self.expect("保留自身 role_key 应更新成功");
        assert_eq!(updated.role_key, key_b);
    }

    /// 更新不存在的角色（或已软删角色）应返回业务错误。
    #[tokio::test]
    async fn update_role_returns_biz_error_when_role_missing() {
        let db = test_db().await;

        let missing = update_role(
            &db,
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
            &db,
            "已删角色",
            &key,
            1,
            Some(chrono::Utc::now().naive_utc()),
        )
        .await;
        let update_deleted = update_role(
            &db,
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
        cleanup(&db, &[deleted.id], &[], &[]).await;
        assert!(
            matches!(update_deleted, Err(AppError::Biz(_))),
            "更新已软删角色应返回 Biz 业务错误，实际：{update_deleted:?}"
        );
    }

    /// 全量更新：主表字段被覆盖，menu_ids / api_ids 全量替换（空数组即清空）。
    #[tokio::test]
    async fn update_role_full_replaces_links() {
        let db = test_db().await;
        let key = unique("keep_links");
        let role_name = unique("full_role");
        let menu_a = seed_menu(&db).await;
        let menu_b = seed_menu(&db).await;
        let api_a = seed_api(&db).await;
        let api_b = seed_api(&db).await;

        let created = create_role(
            &db,
            &CreateRoleReq {
                role_name: role_name.clone(),
                role_key: key.clone(),
                sort: Some(0),
                status: Some(1),
                remark: None,
                menu_ids: Some(vec![menu_a.id, menu_b.id]),
                api_ids: Some(vec![api_a.id, api_b.id]),
            },
        )
        .await
        .expect("创建带关联角色应成功");

        let updated = update_role(
            &db,
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
            .all(&db)
            .await
            .unwrap();
        let api_links = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        assert_eq!(menu_links.len(), 1, "全量替换后只剩新菜单关联");
        assert_eq!(menu_links[0].menu_id, menu_a.id);
        assert_eq!(api_links.len(), 1, "全量替换后只剩新 API 关联");
        assert_eq!(api_links[0].api_id, api_a.id);

        // 传空数组 = 清空关联
        update_role(
            &db,
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
            .all(&db)
            .await
            .unwrap();
        let api_links_cleared = sys_role_api::Entity::find()
            .filter(sys_role_api::Column::RoleId.eq(created.id))
            .all(&db)
            .await
            .unwrap();

        cleanup(
            &db,
            &[created.id],
            &[menu_a.id, menu_b.id],
            &[api_a.id, api_b.id],
        )
        .await;

        assert!(menu_links_cleared.is_empty(), "空数组应清空菜单关联");
        assert!(api_links_cleared.is_empty(), "空数组应清空 API 关联");
    }
}
