//! 文件数据访问（`sys_file`）：函数体由用户按计划任务 4 实现；
//! 下方 `#[cfg(test)]` 集成测试由 AI 编写（TDD 红阶段，实现后应转绿）。
//!
//! 规格依据：docs/superpowers/specs/2026-09-05-w5-file-upload-design.md §3 / §8.1。

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, DatabaseConnection, QueryOrder};

use crate::entity::{sys_file, sys_file::Model};
use crate::modules::file::dto::FileFilter;

/// 按主键查有效记录（排除软删）。
pub async fn find_by_id(db: &DatabaseConnection, id: u64) -> anyhow::Result<Option<Model>> {
    let model = sys_file::Entity::find()
        .filter(sys_file::Column::Id.eq(id))
        .filter(sys_file::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    Ok(model)
}

/// 分页 + 动态过滤（keyword 对 name 模糊），排除软删，created_at 倒序。
pub async fn find_page(
    db: &DatabaseConnection,
    filter: &FileFilter,
    page_index: u64,
    page_size: u64,
) -> anyhow::Result<crate::utils::PageData<Model>> {
    let mut cond = Condition::all();

    if let Some(keyword) = filter.keyword.as_deref() {
        cond = cond.add(sys_file::Column::Name.like(format!("%{}%", keyword)));
    }
    let select = sys_file::Entity::find()
        .filter(cond)
        .filter(sys_file::Column::DeletedAt.is_null())
        .order_by_desc(sys_file::Column::CreatedAt);

    let page = crate::utils::paginate(select, db, page_index, page_size).await?;
    Ok(page)
}

/// 创建记录（上传落库入口）。`actor_id` 为上传人，审计字段由 repo 统一盖章。
pub async fn create_file(
    db: &DatabaseConnection,
    model: sys_file::ActiveModel,
    actor_id: u64,
) -> anyhow::Result<Model> {
    // 创建场景：创建人与更新人同源（写入口径见 AGENTS.md「人字段命名与名称拼装约定」）
    let mut model = model;
    model.created_by = Set(actor_id);
    model.updated_by = Set(actor_id);
    let model = model.insert(db).await?;
    Ok(model)
}

/// 软删单条：`deleted_at` 置为当前时间，返回是否实际删除（不存在或已软删返回 false）。
pub async fn soft_delete_file(db: &DatabaseConnection, id: u64) -> anyhow::Result<bool> {
    if let Some(model) = find_by_id(db, id).await? {
        let mut mode: sys_file::ActiveModel = model.into();
        mode.deleted_at = Set(Some(chrono::Local::now().naive_local()));
        mode.update(db).await?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::sys_file;
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

    /// 造一条文件记录；`stored_name` 用唯一名防测试间冲突（真实 uuid 命名由 service 测试覆盖）。
    #[allow(clippy::too_many_arguments)]
    async fn seed(
        db: &DatabaseConnection,
        name: &str,
        created_at: chrono::NaiveDateTime,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_file::Model {
        let stored_name = format!("{}.txt", unique("stored"));
        sys_file::ActiveModel {
            name: Set(name.to_string()),
            stored_name: Set(stored_name),
            ext: Set("txt".to_string()),
            mime: Set("text/plain".to_string()),
            size: Set(1024),
            created_at: Set(created_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn cleanup(db: &DatabaseConnection, ids: &[u64]) {
        sys_file::Entity::delete_many()
            .filter(sys_file::Column::Id.is_in(ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn find_page_filters_by_keyword_sorts_desc_excludes_deleted() {
        let db = test_db().await;
        let kw = unique("repo_page");
        let base = chrono::Local::now().naive_local();

        // 插入顺序（id 升序）与 created_at 新旧刻意相反：a 最新但 id 最小。
        // 这样「按 created_at 倒序」得到 [a,b,c]，「按 id 倒序」得到 [c,b,a]，
        // 两种实现结果不同，测试才能区分规格要求的排序字段。
        let a = seed(&db, &format!("报告{kw}alpha.txt"), base, None).await;
        let b = seed(
            &db,
            &format!("报告{kw}beta.txt"),
            base - chrono::Duration::seconds(2),
            None,
        )
        .await;
        let c = seed(
            &db,
            &format!("报告{kw}gamma.txt"),
            base - chrono::Duration::seconds(3),
            None,
        )
        .await;
        let deleted = seed(
            &db,
            &format!("报告{kw}delta.txt"),
            base,
            Some(chrono::Local::now().naive_local()),
        )
        .await;
        let other = seed(&db, "其他文件.txt", base, None).await;

        let page = find_page(&db, &FileFilter { keyword: Some(kw) }, 0, 10)
            .await
            .unwrap();

        cleanup(&db, &[a.id, b.id, c.id, deleted.id, other.id]).await;

        assert_eq!(
            page.total, 3,
            "keyword 应只命中 3 条活记录，软删与他名记录不进分页"
        );
        let ids: Vec<u64> = page.items.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec![a.id, b.id, c.id],
            "应按 created_at 倒序（a 最新 id 最小，按 id 倒序的错误实现会得到 c,b,a）"
        );
    }

    #[tokio::test]
    async fn soft_delete_file_marks_deleted_and_second_call_returns_false() {
        let db = test_db().await;
        let kw = unique("repo_del");
        let a = seed(
            &db,
            &format!("{kw}.txt"),
            chrono::Local::now().naive_local(),
            None,
        )
        .await;

        let first = soft_delete_file(&db, a.id).await.unwrap();
        let after = find_by_id(&db, a.id).await.unwrap();
        let second = soft_delete_file(&db, a.id).await.unwrap();

        cleanup(&db, &[a.id]).await;

        assert!(first, "首次软删应返回 true");
        assert!(after.is_none(), "软删后 find_by_id 不可见");
        assert!(!second, "重复软删应返回 false");
    }
}
