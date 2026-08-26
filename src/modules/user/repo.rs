use crate::entity::{
    prelude::SysUser,
    sys_user::{self, Model},
};
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, PaginatorTrait};

pub async fn find_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<Model>> {
    let user = SysUser::find()
        .filter(sys_user::Column::Username.eq(username))
        .one(db)
        .await?;

    Ok(user)
}

/// 分页 + 动态过滤查询（列表接口核心）：
/// - 过滤条件用 `Condition` 动态拼接（有值才 add，无值跳过）
/// - 分页用 `PaginatorTrait::paginate`，`page_index` 为 0-based
/// 返回 `(总条数, 当前页数据)`。
pub async fn find_page(
    db: &DatabaseConnection,
    keyword: Option<String>,
    status: Option<i8>,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<(u64, Vec<Model>)> {
    let mut cond = Condition::all();
    if let Some(kw) = keyword {
        cond = cond.add(sys_user::Column::Username.like(format!("%{kw}%")));
    }
    if let Some(s) = status {
        cond = cond.add(sys_user::Column::Status.eq(s));
    }

    let paginator = SysUser::find().filter(cond).paginate(db, page_size);
    let total = paginator.num_items().await?;
    let items = paginator.fetch_page(page_index).await?;
    Ok((total, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, Database, Set};

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> sea_orm::DatabaseConnection {
        let config = crate::infra::config::Config::load().unwrap();
        Database::connect(&config.database.url).await.unwrap()
    }

    #[tokio::test]
    async fn test_find_by_username_found() {
        let db = test_db().await;
        let username = format!("test_user_{}", std::process::id()); // 唯一名，避免并行冲突

        // 1. 插入测试数据（ActiveModel 写入，NotSet 字段保持默认）
        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("测试用户".to_string()),
            ..Default::default()
        };
        let inserted = model.insert(&db).await.unwrap();

        // 2. 查询并断言
        let found = find_by_username(&db, &username).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, username);

        // 3. 清理（delete_by_id 是 Entity 的方法）
        sys_user::Entity::delete_by_id(inserted.id)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_find_by_username_not_found() {
        let db = test_db().await;
        let found = find_by_username(&db, "definitely_not_exists_xyz")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_page() {
        let db = test_db().await;
        let username = format!("page_user_{}", std::process::id()); // 唯一名

        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            nickname: Set("分页测试".to_string()),
            ..Default::default()
        };
        let inserted = model.insert(&db).await.unwrap();

        // 关键词命中 + 0-based 第 0 页
        let (total, items) = find_page(&db, Some(username.clone()), None, 0, 10)
            .await
            .unwrap();
        assert!(total >= 1);
        assert!(items.iter().any(|u| u.username == username));

        // 关键词不命中
        let (total, _) = find_page(&db, Some("no_such_keyword_xyz".to_string()), None, 0, 10)
            .await
            .unwrap();
        assert_eq!(total, 0);

        sys_user::Entity::delete_by_id(inserted.id)
            .exec(&db)
            .await
            .unwrap();
    }
}
