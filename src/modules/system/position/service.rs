//! 职位业务：分页 / 创建（编码查重）/ 更新（排除自身）/ 查询 / 删除（引用检查）。
//!
//! 写操作走 `*_in_tx` 业务实现 + 对外三行事务入口（同 dept 域）；
//! 需要查库的规则（编码查重、存在性、删除引用检查）在本层，值域校验在 validate 层。

use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, TransactionTrait};

use crate::entity::sys_position;
use crate::modules::system::position::dto::{
    CreatePositionReq, PositionFilter, PositionListReq, UpdatePositionReq,
};
use crate::modules::system::position::repo as position_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 职位分页查询：请求参数（keyword / status / 审计过滤）组装为 repo 过滤条件。
///
/// 不做状态过滤以外的业务判断，纯透传；keyword 同时模糊编码与名称。
pub async fn page_positions(
    db: &impl ConnectionTrait,
    req: &PositionListReq,
) -> Result<PageData<sys_position::Model>, AppError> {
    let filter = PositionFilter {
        keyword: req.keyword.clone(),
        status: req.status,
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
        position_repo::find_position_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 事务内创建职位：编码查重（含软删占位）→ 落库。
///
/// `position_code` 是数据库唯一键且软删行仍占位，查重必须走
/// `find_by_code_include_deleted`（不过滤 deleted_at），否则软删后同编码
/// 会被误判为「可重建」而撞唯一键。
pub async fn create_position_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &CreatePositionReq,
) -> Result<sys_position::Model, AppError> {
    // 1) 编码查重：活的或软删占位命中都拒绝
    if position_repo::find_by_code_include_deleted(txn, &req.position_code)
        .await?
        .is_some()
    {
        return Err(AppError::Biz(format!(
            "职位编码已存在：{}",
            req.position_code
        )));
    }

    // 2) 落库（编码唯一键兜底并发下的重复插入）；审计字段由 repo 统一盖章
    let model = position_repo::create_position_in_tx(
        txn,
        sys_position::ActiveModel {
            position_code: Set(req.position_code.clone()),
            position_name: Set(req.position_name.clone()),
            sort: Set(req.sort),
            status: Set(req.status),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(model)
}

/// 事务内更新职位：判存在 → 编码查重排除自身 → 覆盖业务字段（窄写）。
pub async fn update_position_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdatePositionReq,
) -> Result<sys_position::Model, AppError> {
    // 1) 目标记录必须存在（软删视为不存在）
    if position_repo::find_by_id(txn, req.id).await?.is_none() {
        return Err(AppError::Biz(format!("职位不存在：{}", req.id)));
    }

    // 2) 编码查重排除自身：占用他人编码才拒绝（编辑表单不改编码直接保存须放行）
    if let Some(existing) =
        position_repo::find_by_code_include_deleted(txn, &req.position_code).await?
        && existing.id != req.id
    {
        return Err(AppError::Biz(format!(
            "职位编码已存在：{}",
            req.position_code
        )));
    }

    // 3) 覆盖业务字段（编辑表单整体提交，只 Set 业务列）；审计字段由 repo 统一盖章
    let model = position_repo::update_position_in_tx(
        txn,
        sys_position::ActiveModel {
            id: Set(req.id),
            position_code: Set(req.position_code.clone()),
            position_name: Set(req.position_name.clone()),
            sort: Set(req.sort),
            status: Set(req.status),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(model)
}

/// 事务内删除职位：有用户挂载引用时拒绝，否则软删。
pub async fn delete_position_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    position_id: u64,
) -> Result<(), AppError> {
    if position_repo::find_by_id(txn, position_id).await?.is_none() {
        return Err(AppError::Biz(format!("职位不存在：{}", position_id)));
    }

    // 存在用户挂载引用 → 拒绝（保持任职记录完整性）
    if position_repo::count_user_refs_by_position_id(txn, position_id).await? > 0 {
        return Err(AppError::Biz("职位已被用户挂载，无法删除".to_string()));
    }

    // 软删：命中 0 行说明并发下目标已被删除
    if !position_repo::soft_delete_position_in_tx(txn, position_id, actor_id).await? {
        return Err(AppError::Biz(format!("职位不存在：{}", position_id)));
    }
    Ok(())
}

/// 按 id 查职位详情（软删视为不存在）。
pub async fn get_position(
    db: &impl ConnectionTrait,
    position_id: u64,
) -> Result<sys_position::Model, AppError> {
    let Some(model) = position_repo::find_by_id(db, position_id).await? else {
        return Err(AppError::Biz(format!("职位不存在：{}", position_id)));
    };
    Ok(model)
}

/// 批量按 id 查有效职位（排除软删；停用职位仍返回），供跨域引用校验
/// （如 user 挂载职位存在性）与批量名称拼装。空入参返回空数组。
pub async fn find_by_ids(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> Result<Vec<sys_position::Model>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(position_repo::find_by_ids(db, ids).await?)
}

/// 对外入口：开事务后委托 `create_position_in_tx`，成功后提交。
pub async fn create_position(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &CreatePositionReq,
) -> Result<sys_position::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_position_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 对外入口：开事务后委托 `update_position_in_tx`，成功后提交。
pub async fn update_position(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdatePositionReq,
) -> Result<sys_position::Model, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_position_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 对外入口：开事务后委托 `delete_position_in_tx`，成功后提交。
pub async fn delete_position(
    db: &DatabaseConnection,
    actor_id: u64,
    position_id: u64,
) -> Result<(), AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_position_in_tx(&txn, actor_id, position_id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 既有测试统一用种子 admin（id=1）作 actor。
    const ACTOR_ID: u64 = 1;

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
    async fn test_txn() -> DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    fn now() -> chrono::NaiveDateTime {
        chrono::Local::now().naive_local()
    }

    fn create_req(code: &str) -> CreatePositionReq {
        CreatePositionReq {
            position_code: code.to_string(),
            position_name: format!("职位{code}"),
            sort: 0,
            status: 1,
            remark: String::new(),
        }
    }

    async fn seed_position(
        db: &impl ConnectionTrait,
        code: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_position::Model {
        sys_position::ActiveModel {
            position_code: Set(code.to_string()),
            position_name: Set(format!("职位{code}")),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_user_position_ref(db: &impl ConnectionTrait, position_id: u64, user_id: u64) {
        crate::entity::sys_user_position::ActiveModel {
            user_id: Set(user_id),
            position_id: Set(position_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn create_position_rejects_duplicate_code_including_soft_deleted() {
        let db = test_txn().await;
        let live = seed_position(&db, &unique("dup_live"), 1, None).await;
        let deleted = seed_position(&db, &unique("dup_del"), 1, Some(now())).await;

        let result_live =
            create_position_in_tx(&db, ACTOR_ID, &create_req(&live.position_code)).await;
        let result_deleted =
            create_position_in_tx(&db, ACTOR_ID, &create_req(&deleted.position_code)).await;

        assert!(
            matches!(result_live, Err(AppError::Biz(ref m)) if m.contains("职位编码已存在")),
            "活编码重复应拒绝，实际：{result_live:?}"
        );
        assert!(
            matches!(result_deleted, Err(AppError::Biz(ref m)) if m.contains("职位编码已存在")),
            "软删占位编码重复应拒绝，实际：{result_deleted:?}"
        );
    }

    #[tokio::test]
    async fn create_position_succeeds_with_distinct_code() {
        let db = test_txn().await;
        let code = unique("ok");

        let created = create_position_in_tx(&db, ACTOR_ID, &create_req(&code))
            .await
            .unwrap();

        assert_eq!(created.position_code, code);
        assert_eq!(created.created_by, ACTOR_ID);
        assert_eq!(created.status, 1);
    }

    #[tokio::test]
    async fn update_position_rejects_duplicate_code_excluding_self() {
        let db = test_txn().await;
        let a = seed_position(&db, &unique("upd_a"), 1, None).await;
        let b = seed_position(&db, &unique("upd_b"), 1, None).await;

        // b 想改成 a 的编码 → 拒绝
        let dup = update_position_in_tx(
            &db,
            ACTOR_ID,
            &UpdatePositionReq {
                id: b.id,
                position_code: a.position_code.clone(),
                position_name: "改名".to_string(),
                sort: 0,
                status: 1,
                remark: String::new(),
            },
        )
        .await;
        // b 保留自己的编码 → 放行
        let keep_self = update_position_in_tx(
            &db,
            ACTOR_ID,
            &UpdatePositionReq {
                id: b.id,
                position_code: b.position_code.clone(),
                position_name: "改名".to_string(),
                sort: 0,
                status: 1,
                remark: String::new(),
            },
        )
        .await;

        assert!(
            matches!(dup, Err(AppError::Biz(ref m)) if m.contains("职位编码已存在")),
            "占用他人编码应拒绝，实际：{dup:?}"
        );
        keep_self.expect("保留自身编码应更新成功");
    }

    #[tokio::test]
    async fn update_and_delete_return_biz_error_when_missing_or_soft_deleted() {
        let db = test_txn().await;
        let ghost_id = 9_999_999_999;
        let soft = seed_position(&db, &unique("soft"), 1, Some(now())).await;

        let missing = update_position_in_tx(&db, ACTOR_ID, &create_req_for_update(ghost_id)).await;
        let soft_deleted_update =
            update_position_in_tx(&db, ACTOR_ID, &create_req_for_update(soft.id)).await;
        let delete_missing = delete_position_in_tx(&db, ACTOR_ID, ghost_id).await;
        let delete_soft = delete_position_in_tx(&db, ACTOR_ID, soft.id).await;

        // 四个入口的失败文案统一为「职位不存在：{id}」
        fn biz_msg<T: std::fmt::Debug>(res: &Result<T, AppError>) -> Option<String> {
            match res {
                Err(AppError::Biz(m)) => Some(m.clone()),
                _ => None,
            }
        }
        for (label, res) in [
            ("更新不存在", biz_msg(&missing)),
            ("更新软删", biz_msg(&soft_deleted_update)),
            ("删除不存在", biz_msg(&delete_missing)),
            ("删除软删", biz_msg(&delete_soft)),
        ] {
            let msg = res.unwrap_or_else(|| panic!("{label}应返回业务错误"));
            assert!(
                msg.contains("职位不存在"),
                "{label}应报「职位不存在」，实际：{msg}"
            );
        }
    }

    fn create_req_for_update(id: u64) -> UpdatePositionReq {
        UpdatePositionReq {
            id,
            position_code: unique("upd_miss"),
            position_name: "不存在".to_string(),
            sort: 0,
            status: 1,
            remark: String::new(),
        }
    }

    #[tokio::test]
    async fn delete_position_rejects_when_user_ref_exists() {
        let db = test_txn().await;
        let p = seed_position(&db, &unique("del_ref"), 1, None).await;
        seed_user_position_ref(&db, p.id, 101).await;

        let result = delete_position_in_tx(&db, ACTOR_ID, p.id).await;

        assert!(
            matches!(result, Err(AppError::Biz(ref m)) if m.contains("已被用户挂载")),
            "有用户引用的职位应拒绝删除，实际：{result:?}"
        );
        // 未被删除：仍可查到
        assert!(
            position_repo::find_by_id(&db, p.id)
                .await
                .unwrap()
                .is_some(),
            "拒绝删除后职位应仍然有效"
        );
    }

    #[tokio::test]
    async fn delete_position_soft_deletes_when_no_ref() {
        let db = test_txn().await;
        let p = seed_position(&db, &unique("del_ok"), 1, None).await;

        delete_position_in_tx(&db, ACTOR_ID, p.id).await.unwrap();

        assert!(
            position_repo::find_by_id(&db, p.id)
                .await
                .unwrap()
                .is_none(),
            "删除后 find_by_id 不应再查到"
        );
        assert!(
            position_repo::find_by_code_include_deleted(&db, &p.position_code)
                .await
                .unwrap()
                .is_some(),
            "软删行仍占用编码（含软删占位口径）"
        );
    }

    #[tokio::test]
    async fn get_position_excludes_soft_deleted() {
        let db = test_txn().await;
        let live = seed_position(&db, &unique("get_live"), 1, None).await;
        let soft = seed_position(&db, &unique("get_soft"), 1, Some(now())).await;

        let found = get_position(&db, live.id).await.unwrap();
        let soft_result = get_position(&db, soft.id).await;

        assert_eq!(found.id, live.id);
        assert!(
            matches!(soft_result, Err(AppError::Biz(ref m)) if m.contains("职位不存在")),
            "软删职位 get 应视为不存在，实际：{soft_result:?}"
        );
    }

    #[tokio::test]
    async fn find_by_ids_includes_disabled_and_short_circuits_empty() {
        let db = test_txn().await;
        let enabled = seed_position(&db, &unique("ids_on"), 1, None).await;
        let disabled = seed_position(&db, &unique("ids_off"), 0, None).await;
        let soft = seed_position(&db, &unique("ids_del"), 1, Some(now())).await;

        let empty = find_by_ids(&db, &[]).await.unwrap();
        let mixed = find_by_ids(&db, &[enabled.id, disabled.id, soft.id])
            .await
            .unwrap();

        assert!(empty.is_empty(), "空入参应短路返回空数组");
        assert_eq!(
            mixed.iter().map(|m| m.id).collect::<Vec<_>>(),
            {
                let mut ids = vec![enabled.id, disabled.id];
                ids.sort_unstable();
                ids
            },
            "停用职位应保留，软删职位应排除"
        );
    }
}
