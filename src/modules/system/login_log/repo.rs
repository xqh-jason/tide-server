//! 登录日志数据访问（codegen 生成）。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryOrder, QuerySelect};

use crate::entity::{sys_login_log, sys_login_log::Model};
use crate::modules::system::login_log::dto::LoginLogFilter;

/// 查询单个有效记录（排除软删除）。
pub async fn find_by_id(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<Option<Model>> {
    let m = sys_login_log::Entity::find()
        .filter(sys_login_log::Column::Id.eq(id))
        .filter(sys_login_log::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(m)
}

/// 分页 + 动态过滤查询（username / ip 模糊，status 精确，created_at 倒序）。
pub async fn find_page(
    db: &impl ConnectionTrait,
    filter: &LoginLogFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();
    if let Some(v) = &filter.username {
        cond = cond.add(sys_login_log::Column::Username.like(format!("%{v}%")));
    }
    if let Some(v) = &filter.ip {
        cond = cond.add(sys_login_log::Column::Ip.like(format!("%{v}%")));
    }
    if let Some(v) = &filter.status {
        cond = cond.add(sys_login_log::Column::Status.eq(*v));
    }

    let select = sys_login_log::Entity::find()
        .filter(cond)
        .filter(sys_login_log::Column::DeletedAt.is_null())
        // 统一按主键降序：与其余分页域一致，且 id 唯一且稳定，分页边界确定。
        // （原按 CreatedAt 倒序：同一秒内多行时顺序仍不确定，且无索引时更慢）
        .order_by_desc(sys_login_log::Column::Id);
    crate::utils::paginate(select, db, page_index, page_size).await
}

/// 创建记录（登录 service 落库入口）。
pub async fn create_login_log(
    db: &impl ConnectionTrait,
    model: sys_login_log::ActiveModel,
) -> anyhow::Result<Model> {
    Ok(model.insert(db).await?)
}

/// 软删除：`deleted_at` 置为当前时间。
pub async fn soft_delete_login_log(db: &impl ConnectionTrait, id: u64) -> anyhow::Result<bool> {
    let Some(model) = find_by_id(db, id).await? else {
        return Ok(false);
    };
    let mut model: sys_login_log::ActiveModel = model.into();
    model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
    model.update(db).await?;
    Ok(true)
}

/// 物理删除 created_at 早于 cutoff 的记录（定时清理任务用），返回受影响行数。
pub async fn delete_created_before(
    db: &impl ConnectionTrait,
    cutoff: chrono::NaiveDateTime,
) -> anyhow::Result<u64> {
    const BATCH_SIZE: u64 = 1000;
    let mut total: u64 = 0;
    loop {
        let ids: Vec<u64> = sys_login_log::Entity::find()
            .filter(sys_login_log::Column::CreatedAt.lt(cutoff))
            .order_by_asc(sys_login_log::Column::Id)
            .limit(BATCH_SIZE)
            .all(db)
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect();
        if ids.is_empty() {
            return Ok(total);
        }
        let result = sys_login_log::Entity::delete_many()
            .filter(sys_login_log::Column::Id.is_in(ids))
            .exec(db)
            .await?;
        total += result.rows_affected;
    }
}

/// 批量软删：只处理存在且未删除的行，返回受影响行数。
pub async fn soft_delete_batch(db: &impl ConnectionTrait, ids: &[u64]) -> anyhow::Result<u64> {
    let now = chrono::Local::now().naive_local();
    let result = sys_login_log::Entity::update_many()
        .filter(sys_login_log::Column::Id.is_in(ids.iter().copied()))
        .filter(sys_login_log::Column::DeletedAt.is_null())
        .col_expr(sys_login_log::Column::DeletedAt, Expr::value(Some(now)))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_login_log;
    use sea_orm::{ActiveModelTrait, Database, Set};
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

    #[allow(clippy::too_many_arguments)]
    async fn seed(
        db: &impl ConnectionTrait,
        username: &str,
        ip: &str,
        status: i8,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_login_log::Model {
        sys_login_log::ActiveModel {
            user_id: Set(1),
            username: Set(username.to_string()),
            ip: Set(ip.to_string()),
            agent: Set("test-agent".to_string()),
            status: Set(status),
            msg: Set("登录成功".to_string()),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_page_filters_by_username_ip_status_sorts_desc_excludes_deleted() {
        let db = test_txn().await;
        let kw = unique("repo_page");
        let base = chrono::Local::now().naive_local();
        let deleted_at = Some(chrono::Local::now().naive_local());

        let a = seed(
            &db,
            &format!("repo{kw}alpha"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(3),
            None,
        )
        .await;
        let b = seed(
            &db,
            &format!("repo{kw}beta"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(2),
            None,
        )
        .await;
        let c = seed(
            &db,
            &format!("repo{kw}gamma"),
            "10.0.0.2",
            0,
            base - chrono::Duration::seconds(1),
            None,
        )
        .await;
        let _deleted = seed(
            &db,
            &format!("repo{kw}delta"),
            "10.0.0.1",
            1,
            base,
            deleted_at,
        )
        .await;

        let by_username = find_page(
            &db,
            &LoginLogFilter {
                username: Some(kw.clone()),
                ip: None,
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();
        let by_ip_status = find_page(
            &db,
            &LoginLogFilter {
                username: Some(kw.clone()),
                ip: Some("10.0.0.1".to_string()),
                status: Some(1),
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(by_username.total, 3, "软删记录不应进入分页");
        let ids: Vec<u64> = by_username.items.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![c.id, b.id, a.id], "应按 created_at 倒序");
        assert_eq!(by_ip_status.total, 2, "ip + status 精确过滤");
    }

    #[tokio::test]
    async fn soft_delete_batch_only_marks_alive_rows() {
        let db = test_txn().await;
        let kw = unique("repo_batch");
        let base = chrono::Local::now().naive_local();
        let a = seed(
            &db,
            &format!("repo{kw}a"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(2),
            None,
        )
        .await;
        let b = seed(
            &db,
            &format!("repo{kw}b"),
            "10.0.0.1",
            1,
            base - chrono::Duration::seconds(1),
            None,
        )
        .await;
        let deleted = seed(
            &db,
            &format!("repo{kw}c"),
            "10.0.0.1",
            1,
            base,
            Some(chrono::Local::now().naive_local()),
        )
        .await;

        let affected = soft_delete_batch(&db, &[a.id, b.id, deleted.id])
            .await
            .unwrap();
        let after = find_page(
            &db,
            &LoginLogFilter {
                username: Some(kw.clone()),
                ip: None,
                status: None,
            },
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(affected, 2, "已软删记录不应重复计入");
        assert_eq!(after.total, 0, "批量软删后全部不可见");
    }
}
