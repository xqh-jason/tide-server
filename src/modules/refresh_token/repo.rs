//! 刷新凭证数据访问：只拼 SQL 原语（行有效性过滤 / 盖章 / 分页 / 批删）。
//!
//! 「凭证可用 = 未吊销（revoked_at IS NULL）且未过期（expires_at > NOW）」属于
//! 行级有效性过滤，与「软删过滤」同类，留在 repo；用户是否启用/软删的判定
//! 由调用方（认证中间件 / auth service）基于 JOIN 出的用户行完成。

use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryOrder, QuerySelect};

use crate::entity::{sys_refresh_token, sys_user};
use crate::modules::refresh_token::dto::RefreshTokenFilter;
use sys_refresh_token::Model;

/// 按主键查记录（含已吊销/已过期，供管理端回读与删除前判定）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let record = sys_refresh_token::Entity::find_by_id(id).one(db).await?;
    Ok(record)
}

/// 按记录 id 查「可用凭证 + 用户行」（LEFT JOIN，单条 SQL；认证中间件热路径用）。
///
/// 可用 = 未吊销且未过期；用户行不过滤（启用/软删判定由中间件做），
/// 用户行缺失（理论仅硬删竞态）以 `None` 表示、由调用方视为不可用。
pub async fn find_usable_with_user_by_id(
    db: &impl ConnectionTrait,
    refresh_token_id: u64,
) -> anyhow::Result<Option<(Model, Option<sys_user::Model>)>> {
    find_usable_with_user_by_filter(db, sys_refresh_token::Column::Id.eq(refresh_token_id)).await
}

/// 按 refresh token 哈希查「可用凭证 + 用户行」（刷新端点用，语义同上）。
pub async fn find_usable_with_user_by_hash(
    db: &impl ConnectionTrait,
    refresh_token_hash: &str,
) -> anyhow::Result<Option<(Model, Option<sys_user::Model>)>> {
    find_usable_with_user_by_filter(
        db,
        sys_refresh_token::Column::RefreshTokenHash.eq(refresh_token_hash),
    )
    .await
}

/// 「可用凭证 + 用户行」的共用查询体：行级有效性过滤 + LEFT JOIN 用户。
async fn find_usable_with_user_by_filter(
    db: &impl ConnectionTrait,
    filter: sea_orm::sea_query::SimpleExpr,
) -> anyhow::Result<Option<(Model, Option<sys_user::Model>)>> {
    let now = chrono::Local::now().naive_local();
    let pair = sys_refresh_token::Entity::find()
        .filter(filter)
        .filter(sys_refresh_token::Column::RevokedAt.is_null())
        .filter(sys_refresh_token::Column::ExpiresAt.gt(now))
        .find_also_related(sys_user::Entity)
        .one(db)
        .await?;
    Ok(pair)
}

/// 分页 + 动态过滤查询（username 模糊，online_only 按 last_active_at 窗口，created_at 倒序）。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &RefreshTokenFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(v) = &filter.username {
        cond = cond.add(sys_refresh_token::Column::Username.like(format!("%{v}%")));
    }
    if let Some(true) = filter.online_only {
        let online_since = chrono::Local::now().naive_local()
            - chrono::Duration::minutes(crate::modules::refresh_token::dto::ONLINE_WINDOW_MINUTES);
        cond = cond.add(sys_refresh_token::Column::LastActiveAt.gt(online_since));
    }

    let select = sys_refresh_token::Entity::find()
        .filter(cond)
        .order_by_desc(sys_refresh_token::Column::CreatedAt);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建凭证记录（service 已组装好全部字段，此处纯落库）。
pub async fn create_refresh_token(
    db: &impl ConnectionTrait,
    model: sys_refresh_token::ActiveModel,
) -> anyhow::Result<Model> {
    Ok(model.insert(db).await?)
}

/// 吊销凭证：盖章 revoked_at/revoked_by/revoke_reason，只处理未吊销的行，
/// 返回「是否命中」（存在性判断留给 service）。
pub async fn revoke(
    db: &impl ConnectionTrait,
    refresh_token_id: u64,
    revoked_by: u64,
    reason: &str,
) -> anyhow::Result<bool> {
    let now = chrono::Local::now().naive_local();
    let result = sys_refresh_token::Entity::update_many()
        .filter(sys_refresh_token::Column::Id.eq(refresh_token_id))
        .filter(sys_refresh_token::Column::RevokedAt.is_null())
        .col_expr(sys_refresh_token::Column::RevokedAt, Expr::value(Some(now)))
        .col_expr(
            sys_refresh_token::Column::RevokedBy,
            Expr::value(revoked_by),
        )
        .col_expr(
            sys_refresh_token::Column::RevokeReason,
            Expr::value(reason.to_string()),
        )
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// 回写最后活跃时间（认证中间件 60s 节流后调用）。
pub async fn touch_last_active_at(
    db: &impl ConnectionTrait,
    refresh_token_id: u64,
) -> anyhow::Result<()> {
    let now = chrono::Local::now().naive_local();
    sys_refresh_token::Entity::update_many()
        .filter(sys_refresh_token::Column::Id.eq(refresh_token_id))
        .col_expr(
            sys_refresh_token::Column::LastActiveAt,
            Expr::value(Some(now)),
        )
        .exec(db)
        .await?;
    Ok(())
}

/// 按主键物理删除，返回「是否命中」（service 先行判定死记录，此处不校验状态）。
pub async fn delete_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let result = sys_refresh_token::Entity::delete_by_id(id).exec(db).await?;
    Ok(result.rows_affected > 0)
}

/// 批量物理删除，仅删死记录（已吊销或已过期，SQL 内置 dead 过滤），
/// 活跃记录与不存在的 id 静默跳过，返回受影响行数。
pub async fn delete_batch(db: &impl ConnectionTrait, ids: &[u64]) -> anyhow::Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let now = chrono::Local::now().naive_local();
    let dead = Condition::any()
        .add(sys_refresh_token::Column::RevokedAt.is_not_null())
        .add(sys_refresh_token::Column::ExpiresAt.lte(now));
    let result = sys_refresh_token::Entity::delete_many()
        .filter(sys_refresh_token::Column::Id.is_in(ids.iter().copied()))
        .filter(dead)
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// 物理删除 expires_at 早于 cutoff 的记录（定时清理任务用），返回受影响行数。
pub async fn delete_expired_before(
    db: &impl ConnectionTrait,
    cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<u64> {
    const BATCH_SIZE: u64 = 1000;
    let mut total: u64 = 0;
    loop {
        let ids: Vec<u64> = sys_refresh_token::Entity::find()
            .filter(sys_refresh_token::Column::ExpiresAt.lt(cutoff))
            .order_by_asc(sys_refresh_token::Column::Id)
            .limit(BATCH_SIZE)
            .all(db)
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect();
        if ids.is_empty() {
            return Ok(total);
        }
        let result = sys_refresh_token::Entity::delete_many()
            .filter(sys_refresh_token::Column::Id.is_in(ids))
            .exec(db)
            .await?;
        total += result.rows_affected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_user;
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

    /// 造唯一启用用户（status 走 DB 默认 1），返回 (user_id, username)。
    async fn seed_user(db: &impl ConnectionTrait) -> (u64, String) {
        let username = unique("sess_user");
        let user = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("not-a-real-password".into()),
            nickname: Set("会话测试".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        (user.id, username)
    }

    /// 造凭证记录（直连 ActiveModel，绕过 service），时间为相对当前的偏移。
    #[allow(clippy::too_many_arguments)]
    async fn seed_record(
        db: &impl ConnectionTrait,
        user_id: u64,
        username: &str,
        hash: String,
        expires_in: chrono::Duration,
        last_active_in: chrono::Duration,
        revoked: bool,
    ) -> Model {
        let now = chrono::Local::now().naive_local();
        sys_refresh_token::ActiveModel {
            user_id: Set(user_id),
            username: Set(username.to_string()),
            refresh_token_hash: Set(hash),
            ip: Set("127.0.0.1".into()),
            agent: Set("test-agent".into()),
            last_active_at: Set(now + last_active_in),
            expires_at: Set(now + expires_in),
            revoked_at: Set(revoked.then_some(now)),
            revoke_reason: Set(revoked.then(|| "测试吊销".to_string()).unwrap_or_default()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_refresh_token_persists_model_fields() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let now = chrono::Local::now().naive_local();
        let hash = unique("hash");

        let created = create_refresh_token(
            &db,
            sys_refresh_token::ActiveModel {
                user_id: Set(uid),
                username: Set(username.clone()),
                refresh_token_hash: Set(hash.clone()),
                ip: Set("10.1.1.1".into()),
                agent: Set("agent-x".into()),
                last_active_at: Set(now),
                expires_at: Set(now + chrono::Duration::days(7)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fetched = find_by_id(&db, created.id).await.unwrap().unwrap();
        assert_eq!(fetched.user_id, uid);
        assert_eq!(fetched.username, username);
        assert_eq!(fetched.refresh_token_hash, hash);
        assert_eq!(fetched.ip, "10.1.1.1");
        assert_eq!(fetched.revoked_at, None, "新会话不应处于吊销态");
    }

    /// repo 只过滤会话自身状态（吊销/过期）；用户禁用与否由调用方判定——
    /// 本测试钉死「repo 返回禁用用户的可用会话 + 用户行」这一契约。
    #[tokio::test]
    async fn find_usable_with_user_by_id_filters_only_record_state() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let now = chrono::Local::now().naive_local();

        let active = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            false,
        )
        .await;
        let _revoked = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            true,
        )
        .await;
        let _expired = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::seconds(-1),
            chrono::Duration::zero(),
            false,
        )
        .await;

        let hit = find_usable_with_user_by_id(&db, active.id).await.unwrap();
        let (record, user) = hit.expect("有效凭证应命中");
        assert_eq!(record.id, active.id);
        let user = user.expect("JOIN 应带出用户行（即使被禁用，判定归调用方）");
        assert_eq!(user.id, uid);
        assert_eq!(user.username, username);

        // 吊销 / 过期 / 不存在的会话一律查无
        assert!(
            find_usable_with_user_by_id(&db, _revoked.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            find_usable_with_user_by_id(&db, _expired.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            find_usable_with_user_by_id(&db, 9_999_999_999)
                .await
                .unwrap()
                .is_none()
        );
        let _ = now;
    }

    #[tokio::test]
    async fn find_usable_with_user_by_hash_matches_only_usable_record() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;

        let active_hash = unique("h");
        seed_record(
            &db,
            uid,
            &username,
            active_hash.clone(),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            false,
        )
        .await;
        let revoked_hash = unique("h");
        seed_record(
            &db,
            uid,
            &username,
            revoked_hash.clone(),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            true,
        )
        .await;

        let (session, user) = find_usable_with_user_by_hash(&db, &active_hash)
            .await
            .unwrap()
            .expect("有效哈希应命中");
        assert_eq!(session.user_id, uid);
        assert!(user.is_some());

        assert!(
            find_usable_with_user_by_hash(&db, &revoked_hash)
                .await
                .unwrap()
                .is_none(),
            "已吊销会话即使哈希存在也不可命中"
        );
        assert!(
            find_usable_with_user_by_hash(&db, "ghost-hash")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn find_page_filters_by_username_and_online_only() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let kw = username.as_str();

        // 在线活跃会话（1 分钟前活跃）
        let fresh = seed_record(
            &db,
            uid,
            kw,
            unique("h"),
            chrono::Duration::days(7),
            -chrono::Duration::minutes(1),
            false,
        )
        .await;
        // 不活跃会话（1 小时前活跃，未过期未吊销）
        let _stale = seed_record(
            &db,
            uid,
            kw,
            unique("h"),
            chrono::Duration::days(7),
            -chrono::Duration::hours(1),
            false,
        )
        .await;
        // 别的用户名的会话（不应被 username 模糊命中）
        let _other = seed_record(
            &db,
            uid,
            "someone-else",
            unique("h"),
            chrono::Duration::days(7),
            -chrono::Duration::minutes(1),
            false,
        )
        .await;

        let by_username = find_page(
            &db,
            &RefreshTokenFilter {
                username: Some(kw.to_string()),
                online_only: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        // online 断言按唯一 username 收窄：并行测试走真库 autocommit 会写入
        // 其他 username 的记录，无过滤查询的计数不可作为断言依据
        let online = find_page(
            &db,
            &RefreshTokenFilter {
                username: Some(kw.to_string()),
                online_only: Some(true),
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(by_username.total, 2, "username 模糊应命中 2 条");
        assert_eq!(
            online.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![fresh.id],
            "online_only 只留活跃窗口内的会话"
        );
    }

    #[tokio::test]
    async fn revoke_stamps_once_and_reports_hit() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let record = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            false,
        )
        .await;

        let first = revoke(&db, record.id, 42, "管理员强制下线").await.unwrap();
        assert!(first, "首次吊销应命中");

        let after = find_by_id(&db, record.id).await.unwrap().unwrap();
        assert!(after.revoked_at.is_some(), "吊销时间应盖章");
        assert_eq!(after.revoked_by, 42, "吊销操作人应盖章");
        assert_eq!(after.revoke_reason, "管理员强制下线");

        let second = revoke(&db, record.id, 7, "重复吊销").await.unwrap();
        assert!(!second, "已吊销会话不应重复计入");
        let after2 = find_by_id(&db, record.id).await.unwrap().unwrap();
        assert_eq!(after2.revoked_by, 42, "首次吊销的操作人不被覆盖");
    }

    #[tokio::test]
    async fn touch_last_active_at_refreshes_activity() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;
        let record = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::days(7),
            -chrono::Duration::hours(2),
            false,
        )
        .await;

        touch_last_active_at(&db, record.id).await.unwrap();

        let after = find_by_id(&db, record.id).await.unwrap().unwrap();
        let now = chrono::Local::now().naive_local();
        assert!(
            (now - after.last_active_at) < chrono::Duration::minutes(1),
            "活跃时间应被刷新到当前时刻，实际偏差 {}s",
            (now - after.last_active_at).num_seconds()
        );
    }

    #[tokio::test]
    async fn delete_expired_before_removes_only_expired() {
        let db = test_txn().await;
        let (uid, username) = seed_user(&db).await;

        // 已过期（过期时间 31 天前）+ 吊销但未过期 + 完全有效
        let expired = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            -chrono::Duration::days(31),
            chrono::Duration::zero(),
            false,
        )
        .await;
        let revoked_alive = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            true,
        )
        .await;
        let alive = seed_record(
            &db,
            uid,
            &username,
            unique("h"),
            chrono::Duration::days(7),
            chrono::Duration::zero(),
            false,
        )
        .await;

        let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(30);
        let deleted = delete_expired_before(&db, cutoff).await.unwrap();

        assert_eq!(deleted, 1, "只清理过期超 cutoff 的会话");
        assert!(find_by_id(&db, expired.id).await.unwrap().is_none());
        assert!(
            find_by_id(&db, revoked_alive.id).await.unwrap().is_some(),
            "吊销但未过期的不清（审计保留）"
        );
        assert!(find_by_id(&db, alive.id).await.unwrap().is_some());
    }
}
