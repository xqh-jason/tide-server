//! 权限业务规则层。

use crate::modules::permission::SUPER_ROLE_KEY;
use crate::modules::permission::repo as permission_repo;
use crate::modules::user::repo as user_repo;
use crate::utils::error::AppError;
use sea_orm::ConnectionTrait;

/// 判断用户是否拥有指定操作权限。
///
/// `super` 角色作为项目当前约定拥有全部权限。
pub async fn has_permission(
    db: &impl ConnectionTrait,
    user_id: u64,
    permission_code: &str,
) -> Result<bool, AppError> {
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;
    let role_keys = roles
        .into_iter()
        .map(|role| role.role_key)
        .collect::<Vec<_>>();
    if role_keys.contains(&SUPER_ROLE_KEY.to_string()) {
        return Ok(true);
    }

    let permission_codes = permission_repo::find_permission_codes_by_user_id(db, user_id).await?;
    Ok(permission_codes.contains(&permission_code.to_string()))
}

/// 接口级授权判定：请求 `path + method` 已登记 `sys_api` 时校验角色授权，否则放行。
///
/// 与 [`has_permission`]（按钮权限码）互为双通道：按钮码管「前端按钮显隐 +
/// service 显式校验」，本函数管「接口资源集中拦截」，共同构成
/// 「看得到按钮但调接口被拒」的双保险。
///
/// 判定顺序：
/// 1. 未登记（无生效 API，含软删/停用）→ 放行（fail-open：`sys_api` 空表与
///    新增接口零影响，按域逐步登记接管）；
/// 2. 用户实时有效角色含 `super` → 放行（不信任 JWT 角色快照，与 `has_permission`
///    同一短路口径）；
/// 3. 用户角色与该 API 的 `sys_role_api` 授权有交集 → 放行，否则拒绝。
pub async fn has_api_permission(
    db: &impl ConnectionTrait,
    user_id: u64,
    path: &str,
    method: &str,
) -> Result<bool, AppError> {
    // 未登记该接口，不检查角色授权，放行
    let Some(api_model) = permission_repo::find_active_api_by_path_method(db, path, method).await?
    else {
        return Ok(true);
    };
    let roles = user_repo::find_roles_by_user_id(db, user_id).await?;

    // 是否有超级管理员角色
    if roles
        .iter()
        .any(|role| role.role_key == SUPER_ROLE_KEY.to_string())
    {
        return Ok(true);
    }

    let role_ids = roles.into_iter().map(|role| role.id).collect::<Vec<_>>();
    let has_permission =
        permission_repo::exists_role_api(db, api_model.id, role_ids.as_slice()).await?;

    Ok(has_permission)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_api, sys_role, sys_role_api, sys_user, sys_user_role};
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
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

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    async fn seed_user(db: &impl ConnectionTrait) -> sys_user::Model {
        sys_user::ActiveModel {
            username: Set(unique("apisvc_user")),
            password: Set("x".to_string()),
            nickname: Set("接口授权测试用户".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_role(db: &impl ConnectionTrait) -> sys_role::Model {
        sys_role::ActiveModel {
            role_name: Set(unique("apisvc_role")),
            role_key: Set(unique("apisvc_role_key")),
            sort: Set(0),
            status: Set(1),
            remark: Set(String::new()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
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
            remark: Set("接口授权测试创建".to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_api(
        db: &impl ConnectionTrait,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_api::Model {
        sys_api::ActiveModel {
            // uk_api_path_method 物理唯一（含软删行占位），每行用不同 path
            path: Set(format!("/api/v1/{}/delete", unique("apisvc_api"))),
            method: Set("POST".to_string()),
            description: Set("接口授权测试".to_string()),
            api_group: Set("apisvc_test".to_string()),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
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

    async fn bind_role_api(db: &impl ConnectionTrait, role_id: u64, api_id: u64) {
        sys_role_api::ActiveModel {
            role_id: Set(role_id),
            api_id: Set(api_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn has_api_permission_allows_unregistered_endpoint() {
        let db = test_txn().await;
        let user = seed_user(&db).await;

        let result = has_api_permission(&db, user.id, "/api/v1/not-registered/list", "POST").await;

        assert!(result.unwrap(), "未登记接口应放行（fail-open）");
    }

    #[tokio::test]
    async fn has_api_permission_short_circuits_for_super() {
        let db = test_txn().await;
        let super_role = load_super_role(&db).await;
        let user = seed_user(&db).await;
        bind_user_role(&db, user.id, super_role.id).await;
        let api = seed_api(&db, 1, None).await;

        let result = has_api_permission(&db, user.id, &api.path, &api.method).await;

        assert!(result.unwrap(), "超管应短路放行，无需 sys_role_api 授权");
    }

    #[tokio::test]
    async fn has_api_permission_allows_authorized_role() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let role = seed_role(&db).await;
        bind_user_role(&db, user.id, role.id).await;
        let api = seed_api(&db, 1, None).await;
        bind_role_api(&db, role.id, api.id).await;

        let result = has_api_permission(&db, user.id, &api.path, &api.method).await;

        assert!(result.unwrap(), "已授权角色应放行");
    }

    #[tokio::test]
    async fn has_api_permission_rejects_unauthorized_role() {
        let db = test_txn().await;
        let api = seed_api(&db, 1, None).await;
        let bound_user = seed_user(&db).await;
        let role = seed_role(&db).await;
        bind_user_role(&db, bound_user.id, role.id).await;
        let roleless_user = seed_user(&db).await;

        let bound = has_api_permission(&db, bound_user.id, &api.path, &api.method).await;
        let roleless = has_api_permission(&db, roleless_user.id, &api.path, &api.method).await;

        assert!(!bound.unwrap(), "已绑定角色但未授权该接口应拒绝");
        assert!(!roleless.unwrap(), "无角色用户访问已登记接口应拒绝");
    }

    #[tokio::test]
    async fn has_api_permission_ignores_deleted_or_disabled_api() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let deleted = seed_api(&db, 1, Some(chrono::Local::now().naive_local())).await;
        let disabled = seed_api(&db, 0, None).await;

        let deleted_result = has_api_permission(&db, user.id, &deleted.path, &deleted.method).await;
        let disabled_result =
            has_api_permission(&db, user.id, &disabled.path, &disabled.method).await;

        assert!(deleted_result.unwrap(), "软删 API 等同未登记，应放行");
        assert!(disabled_result.unwrap(), "停用 API 等同未登记，应放行");
    }
}
