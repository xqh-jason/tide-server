//! 用户引用字段名称拼装（W5）：created_by / updated_by 等指向 sys_user 的人字段，
//! 统一在此补齐 `*_name`，前端不做 id → 名称换算。
//!
//! 约定（AGENTS.md「人字段命名与名称拼装约定」）：
//! - 实体实现 `UserRefIds`：收集本记录全部人字段 id；
//! - Resp 实现 `UserRefNames`：按名称映射填充人名字段（查不到给空串）；
//! - `fill_user_names` 是全项目唯一拼装管道：收集 → 一次批量查 → 填充。
//!
//! 本文件当前仅含失败测试（TDD 红阶段），待实现符号：`UserRefIds`、
//! `UserRefNames`、`dedup_ids`、`find_user_name_map_by_ids`、`fill_user_names`。
use crate::entity::{sys_api, sys_dictionary, sys_dictionary_detail, sys_menu, sys_role, sys_user};
use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::utils::error::AppError;

/// 用户引用字段 id 收集器。
pub trait UserRefIds {
    fn user_ref_ids(&self) -> Vec<u64>;
}

/// 用户引用字段名称填充器。
pub trait UserRefNames {
    fn set_user_ref_names(&mut self, names: &HashMap<u64, String>);
}

/// 去重并排序用户引用字段 id。
pub fn dedup_ids(ids: Vec<u64>) -> Vec<u64> {
    if ids.is_empty() {
        return ids;
    }

    let mut ids = ids;
    ids.sort();
    ids.dedup();
    ids
}

pub async fn find_user_name_map_by_ids(
    db: &DatabaseConnection,
    ids: Vec<u64>,
) -> anyhow::Result<HashMap<u64, String>> {
    let ids = dedup_ids(ids);
    let models = sys_user::Entity::find()
        .filter(sys_user::Column::Id.is_in(ids))
        .all(db)
        .await?;
    let mut map = HashMap::new();
    for model in models {
        map.insert(model.id, model.username);
    }
    Ok(map)
}

pub async fn fill_user_names<M: UserRefIds, R: UserRefNames>(
    db: &DatabaseConnection,
    items: Vec<M>,
    convert: impl Fn(M) -> R,
) -> Result<Vec<R>, AppError> {
    // 1. 收集：借用 items（不消费），把每条记录的 (created_by, updated_by)
    //    摊平成扁平的 id 序列
    let ids: Vec<u64> = items
        .iter()
        .flat_map(|m| m.user_ref_ids()) // &M 调方法（自动解引用），Vec<u64> 是 IntoIterator
        .collect();
    let names = find_user_name_map_by_ids(db, ids).await?;
    Ok(items
        .into_iter()
        .map(|m| {
            let mut resp = convert(m); // Model -> Resp
            resp.set_user_ref_names(&names);
            resp
        })
        .collect())
}

impl UserRefIds for sys_user::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_role::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_api::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_menu::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}
impl UserRefIds for sys_dictionary::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_dictionary_detail::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::entity::sys_user;

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

    /// 造一个测试用户，返回 (id, username)。
    async fn seed_user(db: &DatabaseConnection) -> (u64, String) {
        let username = unique("user_ref");
        let model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            ..Default::default()
        };
        let inserted = model.insert(db).await.unwrap();
        (inserted.id, username)
    }

    /// 造一个已软删用户，返回 (id, username)。
    /// 名称解析面向历史引用：操作人即便已软删，历史记录仍应带出名字。
    async fn seed_deleted_user(db: &DatabaseConnection) -> (u64, String) {
        let username = unique("user_ref_del");
        let mut model = sys_user::ActiveModel {
            username: Set(username.clone()),
            password: Set("x".to_string()),
            ..Default::default()
        };
        model.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        let inserted = model.insert(db).await.unwrap();
        (inserted.id, username)
    }

    /// 测试辅助：一条带人字段的假记录（模拟实体侧实现 UserRefIds）。
    struct AuditRow {
        created_by: u64,
        updated_by: u64,
    }

    impl UserRefIds for AuditRow {
        fn user_ref_ids(&self) -> Vec<u64> {
            vec![self.created_by, self.updated_by]
        }
    }

    /// 测试辅助：一个带 *_name 字段的假 Resp。
    struct AuditResp {
        created_by: u64,
        updated_by: u64,
        created_by_name: String,
        updated_by_name: String,
    }

    impl UserRefNames for AuditResp {
        fn set_user_ref_names(&mut self, names: &HashMap<u64, String>) {
            self.created_by_name = names.get(&self.created_by).cloned().unwrap_or_default();
            self.updated_by_name = names.get(&self.updated_by).cloned().unwrap_or_default();
        }
    }

    #[test]
    fn dedup_ids_sorts_and_removes_duplicates() {
        assert_eq!(dedup_ids(vec![3, 1, 3, 2, 1]), vec![1, 2, 3]);
        assert_eq!(dedup_ids(vec![5]), vec![5]);
        assert!(dedup_ids(Vec::<u64>::new()).is_empty());
    }

    #[tokio::test]
    async fn find_user_name_map_returns_username() {
        let db = test_db().await;
        let (user_a, a_name) = seed_user(&db).await;
        let (user_b, b_name) = seed_user(&db).await;

        let map = find_user_name_map_by_ids(&db, vec![user_a, user_b])
            .await
            .unwrap();

        assert_eq!(map.get(&user_a), Some(&a_name), "显示名取 username");
        assert_eq!(map.get(&user_b), Some(&b_name));

        sys_user::Entity::delete_by_id(user_a)
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(user_b)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn find_user_name_map_includes_soft_deleted_for_history() {
        let db = test_db().await;
        let (deleted_id, deleted_name) = seed_deleted_user(&db).await;

        let map = find_user_name_map_by_ids(&db, vec![deleted_id])
            .await
            .unwrap();

        assert_eq!(
            map.get(&deleted_id),
            Some(&deleted_name),
            "名称解析面向历史引用：软删用户也要能带出名字"
        );
    }

    #[tokio::test]
    async fn find_user_name_map_empty_input_returns_empty_map() {
        let db = test_db().await;
        let map = find_user_name_map_by_ids(&db, vec![]).await.unwrap();
        assert!(map.is_empty(), "空入参不应发查询");
    }

    #[tokio::test]
    async fn fill_user_names_fills_all_records_in_one_batch() {
        let db = test_db().await;
        let (a_id, a_name) = seed_user(&db).await;
        let (b_id, b_name) = seed_user(&db).await;

        let rows = vec![
            AuditRow {
                created_by: a_id,
                updated_by: b_id,
            },
            AuditRow {
                created_by: b_id,
                updated_by: a_id,
            },
        ];
        let resps = fill_user_names(&db, rows, |r| AuditResp {
            created_by: r.created_by,
            updated_by: r.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        })
        .await
        .unwrap();

        assert_eq!(resps.len(), 2);
        assert_eq!(resps[0].created_by_name, a_name);
        assert_eq!(resps[0].updated_by_name, b_name);
        assert_eq!(resps[1].created_by_name, b_name);
        assert_eq!(resps[1].updated_by_name, a_name);

        sys_user::Entity::delete_by_id(a_id)
            .exec(&db)
            .await
            .unwrap();
        sys_user::Entity::delete_by_id(b_id)
            .exec(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn fill_user_names_missing_user_yields_empty_string() {
        let db = test_db().await;
        let (a_id, a_name) = seed_user(&db).await;
        let ghost = 9_999_999_999;

        let rows = vec![AuditRow {
            created_by: a_id,
            updated_by: ghost,
        }];
        let resps = fill_user_names(&db, rows, |r| AuditResp {
            created_by: r.created_by,
            updated_by: r.updated_by,
            created_by_name: String::new(),
            updated_by_name: String::new(),
        })
        .await
        .unwrap();

        assert_eq!(resps[0].created_by_name, a_name, "存在的用户正常填充");
        assert_eq!(resps[0].updated_by_name.len(), 0, "不存在的用户给空串");

        sys_user::Entity::delete_by_id(a_id)
            .exec(&db)
            .await
            .unwrap();
    }
}
