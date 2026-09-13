//! 刷新凭证业务：创建（生成明文 + 哈希落库）、分页、强制下线（吊销）、物理删除。

use sea_orm::ActiveValue::Set;
use sea_orm::ConnectionTrait;

use crate::entity::sys_refresh_token;
use crate::modules::refresh_token::dto::{CreateRefreshTokenParams, RefreshTokenListReq};
use crate::modules::refresh_token::repo as refresh_token_repo;
use crate::utils::PageData;
use crate::utils::crypt;
use crate::utils::error::AppError;
use uuid::Uuid;

/// 登录成功后创建凭证记录：生成一次性明文 refresh token（uuid v4 simple，32 位 hex），
/// SHA-256 哈希落库；明文只返回一次，由 auth handler 设进 HttpOnly Cookie。
pub async fn create_refresh_token(
    db: &impl ConnectionTrait,
    params: CreateRefreshTokenParams,
) -> Result<(sys_refresh_token::Model, String), AppError> {
    let plaintext = Uuid::new_v4().simple().to_string();
    let model = sys_refresh_token::ActiveModel {
        user_id: Set(params.user_id),
        username: Set(params.username),
        refresh_token_hash: Set(crypt::sha256_hex(&plaintext)),
        ip: Set(params.ip),
        agent: Set(params.agent),
        expires_at: Set(chrono::Local::now().naive_local()
            + chrono::Duration::seconds(params.refresh_ttl_seconds)),
        ..Default::default()
    };
    let record = refresh_token_repo::create_refresh_token(db, model).await?;
    Ok((record, plaintext))
}

/// 分页查询凭证记录（管理端在线会话/历史列表）。
pub async fn page_refresh_tokens(
    db: &impl ConnectionTrait,
    req: &RefreshTokenListReq,
) -> anyhow::Result<PageData<sys_refresh_token::Model>> {
    refresh_token_repo::find_page(
        db,
        &crate::modules::refresh_token::dto::RefreshTokenFilter {
            username: req.username.clone(),
            online_only: req.online_only,
        },
        req.page.page_index(),
        req.page.page_size(),
    )
    .await
}

/// 强制下线（吊销凭证并盖章，保留审计痕迹；登出复用同一语义）：
/// 记录不存在或已吊销返回业务错误；`actor_id` = 0 表示本人登出，> 0 为管理员。
pub async fn force_logout_refresh_token(
    db: &impl ConnectionTrait,
    refresh_token_id: u64,
    actor_id: u64,
    reason: &str,
) -> Result<(), AppError> {
    let hit = refresh_token_repo::revoke(db, refresh_token_id, actor_id, reason).await?;
    if !hit {
        return Err(AppError::Biz("会话不存在或已下线".into()));
    }
    Ok(())
}

/// 物理删除单条凭证记录（管理端清理历史）：
/// 仅允许删除死记录（已吊销或已过期）；仍在线的会话必须先强制下线，
/// 防止「无痕踢人」绕过吊销审计（spec §4.4）。
pub async fn delete_refresh_token(
    db: &impl ConnectionTrait,
    refresh_token_id: u64,
) -> Result<(), AppError> {
    let Some(record) = refresh_token_repo::find_by_id(db, refresh_token_id).await? else {
        return Err(AppError::Biz(format!("刷新凭证不存在：{refresh_token_id}")));
    };
    let now = chrono::Local::now().naive_local();
    if record.revoked_at.is_none() && record.expires_at > now {
        return Err(AppError::Biz("会话仍在线，请先强制下线".into()));
    }
    refresh_token_repo::delete_by_id(db, refresh_token_id).await?;
    Ok(())
}

/// 批量物理删除：活跃记录静默跳过（SQL 内置 dead 过滤），返回受影响行数。
pub async fn delete_refresh_token_batch(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> Result<u64, AppError> {
    if ids.is_empty() {
        return Ok(0);
    }
    Ok(refresh_token_repo::delete_batch(db, ids).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{sys_refresh_token, sys_user};
    use crate::modules::refresh_token::dto::RefreshTokenFilter;
    use crate::utils::PageQuery;
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
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

    /// 造唯一启用用户，返回 (user_id, username)。
    async fn seed_user(db: &impl ConnectionTrait) -> (u64, String) {
        let username = unique("sess_svc_user");
        let user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("not-a-real-password".into()),
            nickname: Set("会话服务测试".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        (user.id, username)
    }

    fn list_req(username: Option<String>, online_only: Option<bool>) -> RefreshTokenListReq {
        RefreshTokenListReq {
            page: PageQuery {
                page: None,
                page_size: None,
            },
            username,
            online_only,
        }
    }

    #[tokio::test]
    async fn create_refresh_token_generates_unique_plaintext_and_persists_hash() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;

        let (first, plaintext_a) = create_refresh_token(
            &db,
            CreateRefreshTokenParams {
                user_id: uid,
                username: username.clone(),
                ip: "127.0.0.1".into(),
                agent: "test-agent".into(),
                refresh_ttl_seconds: 604_800,
            },
        )
        .await
        .unwrap();
        let (second, plaintext_b) = create_refresh_token(
            &db,
            CreateRefreshTokenParams {
                user_id: uid,
                username: username.clone(),
                ip: "127.0.0.1".into(),
                agent: "test-agent".into(),
                refresh_ttl_seconds: 604_800,
            },
        )
        .await
        .unwrap();

        // 明文：32 位 hex（uuid simple），两次生成互不相同
        assert_eq!(
            plaintext_a.len(),
            32,
            "uuid simple 应为 32 位 hex：{plaintext_a}"
        );
        assert!(plaintext_a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(plaintext_a, plaintext_b, "每次登录应生成新明文");

        // 落库的是 SHA-256 哈希而非明文，且过期时间 = 登录时刻 + ttl
        for (model, plaintext) in [
            (&first, plaintext_a.as_str()),
            (&second, plaintext_b.as_str()),
        ] {
            assert_eq!(model.user_id, uid);
            assert_eq!(model.username, username);
            assert_eq!(
                model.refresh_token_hash,
                crate::utils::crypt::sha256_hex(plaintext),
                "落库应为明文的 SHA-256 hex"
            );
            assert_eq!(model.revoked_at, None);
            let expected = chrono::Local::now().naive_local() + chrono::Duration::seconds(604_800);
            assert!(
                (model.expires_at - expected).num_seconds().abs() < 5,
                "expires_at 应为 now + refresh_ttl"
            );
        }
        assert_ne!(first.id, second.id);
    }

    #[tokio::test]
    async fn page_refresh_tokens_passes_filters() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let now = chrono::Local::now().naive_local();

        let fresh = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now + chrono::Duration::days(7)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let _stale = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now - chrono::Duration::hours(2)),
            expires_at: Set(now + chrono::Duration::days(7)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let by_username = page_refresh_tokens(&db, &list_req(Some(username.clone()), None))
            .await
            .unwrap();
        // online 断言按唯一 username 收窄：并行测试走真库 autocommit 会写入
        // 其他 username 的记录，无过滤查询的计数不可作为断言依据
        let online = page_refresh_tokens(&db, &list_req(Some(username), Some(true)))
            .await
            .unwrap();

        assert_eq!(by_username.total, 2);
        assert_eq!(
            online.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![fresh.id]
        );
    }

    #[tokio::test]
    async fn force_logout_refresh_token_stamps_actor_and_reason_once() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let now = chrono::Local::now().naive_local();
        let record = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now + chrono::Duration::days(7)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        force_logout_refresh_token(&db, record.id, 42, "管理员强制下线")
            .await
            .expect("存活的凭证应可强制下线");

        let after = crate::modules::refresh_token::repo::find_by_id(&db, record.id)
            .await
            .unwrap()
            .unwrap();
        assert!(after.revoked_at.is_some(), "强制下线应盖章吊销时间");
        assert_eq!(after.revoked_by, 42, "revoked_by 应为操作管理员 id");
        assert_eq!(after.revoke_reason, "管理员强制下线");

        // 已吊销的记录再次下线 → 业务错误
        let second = force_logout_refresh_token(&db, record.id, 42, "重复下线").await;
        assert!(
            matches!(second, Err(AppError::Biz(_))),
            "已吊销记录重复下线应报业务错误，实际 {second:?}"
        );
    }

    #[tokio::test]
    async fn force_logout_missing_record_returns_biz_error() {
        let db = test_txn().await;
        let err = force_logout_refresh_token(&db, 9_999_999_999, 42, "管理员强制下线").await;
        assert!(
            matches!(err, Err(AppError::Biz(_))),
            "不存在的记录应报业务错误"
        );
        // filter 结构体在此占位引用，防止实现前 dead code 误报干扰断言阅读
        let _ = RefreshTokenFilter::default();
    }

    #[tokio::test]
    async fn delete_refresh_token_removes_dead_but_rejects_alive() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let now = chrono::Local::now().naive_local();

        // 已吊销（死）记录：可直接物理删除
        let revoked = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now + chrono::Duration::days(7)),
            revoked_at: Set(Some(now)),
            revoke_reason: Set("管理员强制下线".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // 已过期（死）记录：同样可删
        let expired = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now - chrono::Duration::seconds(1)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        // 仍在线（活）记录：拒绝物理删除
        let alive = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now + chrono::Duration::days(7)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        delete_refresh_token(&db, alive.id).await.unwrap_err();
        let alive_err = delete_refresh_token(&db, alive.id).await.unwrap_err();
        assert!(
            matches!(&alive_err, AppError::Biz(msg) if msg.contains("强制下线")),
            "活跃记录应拒绝物理删除并提示先强制下线，实际 {alive_err:?}"
        );

        delete_refresh_token(&db, revoked.id).await.unwrap();
        delete_refresh_token(&db, expired.id).await.unwrap();
        assert!(
            crate::modules::refresh_token::repo::find_by_id(&db, revoked.id)
                .await
                .unwrap()
                .is_none(),
            "已吊销记录应被物理删除"
        );
        assert!(
            crate::modules::refresh_token::repo::find_by_id(&db, expired.id)
                .await
                .unwrap()
                .is_none(),
            "已过期记录应被物理删除"
        );
        assert!(
            crate::modules::refresh_token::repo::find_by_id(&db, alive.id)
                .await
                .unwrap()
                .is_some(),
            "活跃记录不应被删除"
        );
    }

    #[tokio::test]
    async fn delete_refresh_token_missing_returns_biz_error() {
        let db = test_txn().await;
        let err = delete_refresh_token(&db, 9_999_999_999).await;
        assert!(
            matches!(err, Err(AppError::Biz(_))),
            "不存在的记录应报业务错误"
        );
    }

    #[tokio::test]
    async fn delete_refresh_token_batch_skips_alive_and_counts_dead() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let now = chrono::Local::now().naive_local();

        let dead = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now - chrono::Duration::seconds(1)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let alive = sys_refresh_token::ActiveModel {
            user_id: Set(uid),
            username: Set(username.clone()),
            refresh_token_hash: Set(unique("h")),
            ip: Set("127.0.0.1".into()),
            agent: Set("a".into()),
            last_active_at: Set(now),
            expires_at: Set(now + chrono::Duration::days(7)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let empty = delete_refresh_token_batch(&db, &[]).await.unwrap();
        let affected = delete_refresh_token_batch(&db, &[dead.id, alive.id, 9_999_999_999])
            .await
            .unwrap();

        assert_eq!(empty, 0, "空数组应返回 0 且不执行删除");
        assert_eq!(affected, 1, "活跃记录与不存在的 id 应被静默跳过");
        assert!(
            crate::modules::refresh_token::repo::find_by_id(&db, dead.id)
                .await
                .unwrap()
                .is_none(),
            "死记录应被物理删除"
        );
        assert!(
            crate::modules::refresh_token::repo::find_by_id(&db, alive.id)
                .await
                .unwrap()
                .is_some(),
            "活跃记录不应被删除"
        );
    }
}
