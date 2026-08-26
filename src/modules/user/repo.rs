use crate::entity::{
    prelude::SysUser,
    sys_user::{self, Model},
};
use sea_orm::entity::prelude::*;

pub async fn find_by_username(
    db: &sea_orm::DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<Model>> {
    let user = SysUser::find()
        .filter(sys_user::Column::Username.eq(username))
        .one(db)
        .await?;

    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, Database, Set};

    /// 测试数据库连接：读 config.toml 连真库（需 MySQL 运行：docker compose up -d）
    async fn test_db() -> sea_orm::DatabaseConnection {
        let config = crate::config::Config::load().unwrap();
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
}
