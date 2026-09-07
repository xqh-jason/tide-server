//! 系统配置业务：键值参数 CRUD（key 查重含软删占位）与网站设置（单行 id=1）。
//!
//! 审计字段由 repo 统一盖章，service 只透传 actor_id（AGENTS.md 写入口径）。
//! 规格依据：docs/superpowers/specs/2026-09-05-w5-config-design.md §4 / §6。

use sea_orm::ActiveValue::Set;
use sea_orm::ConnectionTrait;

use crate::entity::{sys_config, sys_site_config};
use crate::modules::config::dto::{
    ConfigFilter, ConfigListReq, CreateConfigReq, UpdateConfigReq, UpdateSiteConfigReq,
};
use crate::modules::config::repo as config_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

// —— 键值参数 ——

/// 参数分页（keyword 透传 repo）。
pub async fn page_configs(
    db: &impl ConnectionTrait,
    req: &ConfigListReq,
) -> Result<PageData<sys_config::Model>, AppError> {
    let filter = ConfigFilter {
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
    };
    Ok(
        config_repo::find_config_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 创建参数：config_key 全局唯一（含软删占位查重）。
pub async fn create_config(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &CreateConfigReq,
) -> Result<sys_config::Model, AppError> {
    // 查重含软删占位：键名删除后仍视为已占用（与 dictionary 的 type 同语义）
    if config_repo::find_config_by_key_include_deleted(db, &req.config_key)
        .await?
        .is_some()
    {
        return Err(AppError::Biz(format!("配置键已存在：{}", req.config_key)));
    }
    // created_by/updated_by 不在此处写：审计字段由 repo 统一盖章（AGENTS.md 写入口径）
    Ok(config_repo::create_config(
        db,
        sys_config::ActiveModel {
            config_name: Set(req.config_name.clone()),
            config_key: Set(req.config_key.clone()),
            config_value: Set(req.config_value.clone()),
            remark: Set(req.remark.clone().unwrap_or_default()),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 更新参数：记录必须存在；key 查重需排除自身。
pub async fn update_config(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &UpdateConfigReq,
) -> Result<sys_config::Model, AppError> {
    // 记录必须存在且未软删
    config_repo::find_config_by_id(db, req.id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("配置不存在：{}", req.id)))?;
    // key 查重排除自身：改键撞到别的行（含软删占位）→ 拒绝
    if let Some(hit) = config_repo::find_config_by_key_include_deleted(db, &req.config_key).await? {
        if hit.id != req.id {
            return Err(AppError::Biz(format!("配置键已存在：{}", req.config_key)));
        }
    }
    Ok(config_repo::update_config(
        db,
        sys_config::ActiveModel {
            id: Set(req.id),
            config_name: Set(req.config_name.clone()),
            config_key: Set(req.config_key.clone()),
            config_value: Set(req.config_value.clone()),
            remark: Set(req.remark.clone().unwrap_or_default()),
            ..Default::default()
        },
        actor_id,
    )
    .await?)
}

/// 参数详情（不存在 → Biz("配置不存在：{id}")）。
pub async fn get_config(db: &impl ConnectionTrait, id: u64) -> Result<sys_config::Model, AppError> {
    let model = config_repo::find_config_by_id(db, id).await?;
    model.ok_or_else(|| AppError::Biz(format!("配置不存在：{}", id)))
}

/// 删除参数：软删（repo 返回 false 即不存在或已软删 → Biz）。
pub async fn delete_config(db: &impl ConnectionTrait, id: u64) -> Result<(), AppError> {
    if !config_repo::soft_delete_config(db, id).await? {
        return Err(AppError::Biz(format!("配置不存在：{}", id)));
    }
    Ok(())
}

// —— 网站设置 ——

/// 网站设置默认值：行缺失（迁移未跑等异常）时兜底，保证公开端点不报错。
/// 注意仅内存兜底，不落库——补齐行应执行迁移种子，而非接口隐式写。
fn default_site_config() -> sys_site_config::Model {
    sys_site_config::Model {
        id: 1,
        name: String::new(),
        logo: String::new(),
        ico: String::new(),
        watermark_text: String::new(),
        watermark_enable: 0,
        watermark_type: "text".into(),
        watermark_pic: String::new(),
        mode: "white".into(),
        side_mode: "dark".into(),
        color: "#409EFF".into(),
        created_by: 0,
        updated_by: 0,
        created_at: chrono::Local::now().naive_local(),
        updated_at: chrono::Local::now().naive_local(),
        deleted_at: None,
    }
}

/// 网站设置：查单行（None 时回退默认值）。
pub async fn get_site_config(
    db: &impl ConnectionTrait,
) -> Result<sys_site_config::Model, AppError> {
    Ok(config_repo::find_site_config(db)
        .await?
        .unwrap_or_else(default_site_config))
}

/// 网站设置：全量更新 id=1 行（行缺失 → Biz("网站设置记录缺失，请执行迁移种子")）。
pub async fn update_site_config(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &UpdateSiteConfigReq,
) -> Result<sys_site_config::Model, AppError> {
    // 行由迁移种子保障；缺失属环境异常，明确报错而非隐式创建
    let model = config_repo::find_site_config(db)
        .await?
        .ok_or_else(|| AppError::Biz("网站设置记录缺失，请执行迁移种子".into()))?;
    let mut model: sys_site_config::ActiveModel = model.into();
    model.name = Set(req.name.clone());
    model.logo = Set(req.logo.clone());
    model.ico = Set(req.ico.clone());
    model.watermark_text = Set(req.watermark_text.clone());
    model.watermark_enable = Set(req.watermark_enable);
    model.watermark_type = Set(req.watermark_type.clone());
    model.watermark_pic = Set(req.watermark_pic.clone());
    model.mode = Set(req.mode.clone());
    model.side_mode = Set(req.side_mode.clone());
    model.color = Set(req.color.clone());
    Ok(config_repo::update_site_config(db, model, actor_id).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_config;
    use sea_orm::{ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter};
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

    fn create_req(key: &str) -> CreateConfigReq {
        CreateConfigReq {
            config_name: format!("参数{key}"),
            config_key: key.to_string(),
            config_value: "v".into(),
            remark: None,
        }
    }

    /// 清理：连软删行一并硬删，避免唯一键残留影响其他测试。
    async fn cleanup(db: &impl ConnectionTrait, ids: &[u64]) {
        sys_config::Entity::delete_many()
            .filter(sys_config::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_config_rejects_duplicate_key_including_deleted() {
        let db = test_txn().await;
        let key = unique("svc_cfg");
        let actor = 1;

        let first = create_config(&db, actor, &create_req(&key)).await.unwrap();
        let dup = create_config(&db, actor, &create_req(&key)).await;
        // 软删后键名仍占位：再建同键依旧被拒
        delete_config(&db, first.id).await.unwrap();
        let dup_after_delete = create_config(&db, actor, &create_req(&key)).await;

        assert!(
            matches!(dup,
            Err(AppError::Biz(ref m)) if m.contains("配置键已存在")),
            "实际 {dup:?}"
        );
        assert!(
            matches!(dup_after_delete,
            Err(AppError::Biz(ref m)) if m.contains("配置键已存在")),
            "软删占位应拦同键，实际 {dup_after_delete:?}"
        );
    }

    #[tokio::test]
    async fn update_config_allows_own_key_rejects_foreign_key() {
        let db = test_txn().await;
        let key1 = unique("svc_k1");
        let key2 = unique("svc_k2");
        let actor = 1;
        let a = create_config(&db, actor, &create_req(&key1)).await.unwrap();
        let _b = create_config(&db, actor, &create_req(&key2)).await.unwrap();

        // key 改成 b 的键 → Biz
        let conflict = UpdateConfigReq {
            id: a.id,
            config_name: a.config_name.clone(),
            config_key: key2.clone(),
            config_value: "v2".into(),
            remark: None,
        };
        let foreign = update_config(&db, actor, &conflict).await;
        // key 保持自身，其余字段更新 → 成功且值生效
        let own = UpdateConfigReq {
            id: a.id,
            config_name: "改名".into(),
            config_key: key1.clone(),
            config_value: "v3".into(),
            remark: None,
        };
        let own_ok = update_config(&db, actor, &own).await.unwrap();

        assert!(
            matches!(foreign,
            Err(AppError::Biz(ref m)) if m.contains("配置键已存在")),
            "实际 {foreign:?}"
        );
        assert_eq!(own_ok.config_name, "改名");
        assert_eq!(own_ok.config_value, "v3");
        assert_eq!(own_ok.updated_by, actor, "更新人应盖章为 actor");
    }

    #[tokio::test]
    async fn get_and_delete_missing_return_biz_error() {
        let db = test_txn().await;

        let get_missing = get_config(&db, 9_999_999_999).await;
        let delete_missing = delete_config(&db, 9_999_999_999).await;

        assert!(
            matches!(get_missing,
            Err(AppError::Biz(ref m)) if m.contains("配置不存在")),
            "实际 {get_missing:?}"
        );
        assert!(
            matches!(delete_missing,
            Err(AppError::Biz(ref m)) if m.contains("配置不存在")),
            "实际 {delete_missing:?}"
        );
    }

    #[tokio::test]
    async fn site_config_get_returns_seed_and_update_stamps_actor() {
        let db = test_txn().await;

        let seeded = get_site_config(&db).await.unwrap();
        assert_eq!(seeded.id, 1, "种子行应存在（迁移保障）");
        let original_name = seeded.name.clone();

        // 全量更新：改名并盖章 actor=42，再读应生效
        let req = UpdateSiteConfigReq {
            name: format!("临时站点_{}", unique("site")),
            logo: String::new(),
            ico: String::new(),
            watermark_text: String::new(),
            watermark_enable: 0,
            watermark_type: "text".into(),
            watermark_pic: String::new(),
            mode: "white".into(),
            side_mode: "dark".into(),
            color: "#409EFF".into(),
        };
        let updated = update_site_config(&db, 42, &req).await.unwrap();
        let reread = get_site_config(&db).await.unwrap();

        // 恢复种子行展示名，避免污染默认站点设置
        let restore = UpdateSiteConfigReq {
            name: original_name,
            ..req.clone()
        };
        update_site_config(&db, 0, &restore).await.unwrap();

        assert_eq!(updated.name, req.name);
        assert_eq!(reread.name, req.name, "更新后 get 应读到新值");
        assert_eq!(updated.updated_by, 42, "更新人应盖章为 actor");
    }
}
