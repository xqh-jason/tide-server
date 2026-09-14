//! 数据字典业务：类型表与字典项表。
//!
//! 错误文案口径：
//! - `字典类型不存在：{id}` / `字典项不存在：{id}`
//! - `字典类型编码已存在：{type}` / `字典值已存在：{value}`
//! - `字典类型不存在或已禁用：{type}`

use sea_orm::ActiveValue::Set;
use sea_orm::entity::prelude::*;
use sea_orm::{DatabaseTransaction, TransactionTrait};

use crate::entity::{sys_dictionary, sys_dictionary_detail};
use crate::modules::system::dictionary::dto::{
    CreateDictionaryDetailReq, CreateDictionaryReq, DictionaryDetailFilter,
    DictionaryDetailListReq, DictionaryFilter, DictionaryListReq, UpdateDictionaryDetailReq,
    UpdateDictionaryReq,
};
use crate::modules::system::dictionary::repo as dict_repo;
use crate::utils::PageData;
use crate::utils::error::AppError;

// —— 字典类型 ——

/// 字典类型分页查询：请求参数（keyword / status / 审计过滤）组装为 repo 过滤条件。
///
/// 不做状态过滤以外的业务判断，纯透传；keyword 同时模糊 name 与 type。
pub async fn page_dictionaries(
    db: &impl ConnectionTrait,
    req: &DictionaryListReq,
) -> Result<PageData<sys_dictionary::Model>, AppError> {
    let filter = DictionaryFilter {
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
        dict_repo::find_dictionary_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 创建类型：type 查重（含软删占位）→ 落库。
///
/// 唯一性口径：`type` 是数据库唯一键，软删行仍占位，
/// 因此查重必须走 `*_include_deleted`（不过滤 deleted_at），
/// 否则同一 type 在软删后会被误判为「可重建」而撞唯一键。
pub async fn create_dictionary(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &CreateDictionaryReq,
) -> Result<sys_dictionary::Model, AppError> {
    // 1) type 查重：活的或软删占位命中都拒绝
    if dict_repo::find_dictionary_by_type_include_deleted(db, &req.r#type)
        .await?
        .is_some()
    {
        return Err(AppError::Biz(format!("字典类型编码已存在：{}", req.r#type)));
    }

    // 2) 落库（type 唯一键兜底并发下的重复插入）；审计字段由 repo 统一盖章
    let result = dict_repo::create_dictionary(
        db,
        sys_dictionary::ActiveModel {
            name: Set(req.name.clone()),
            r#type: Set(req.r#type.clone()),
            status: Set(req.status),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(result)
}

/// 更新类型：判存在 → type 查重排除自身 → 全量覆盖。
///
/// 「排除自身」是编辑表单的常见陷阱：用户不改 type 直接保存时，
/// 查重命中的正是本行，必须放行——只有 `existing.id != req.id` 才算真重复。
pub async fn update_dictionary(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &UpdateDictionaryReq,
) -> Result<sys_dictionary::Model, AppError> {
    // 1) 目标记录必须存在（软删视为不存在）
    if dict_repo::find_dictionary_by_id(db, req.id)
        .await?
        .is_none()
    {
        return Err(AppError::Biz(format!("字典类型不存在：{}", req.id)));
    }

    // 2) type 查重排除自身：占用他人 type 才拒绝
    if let Some(existing) =
        dict_repo::find_dictionary_by_type_include_deleted(db, &req.r#type).await?
        && existing.id != req.id
    {
        return Err(AppError::Biz(format!("字典类型编码已存在：{}", req.r#type)));
    }

    // 3) 全量覆盖更新（编辑表单整体提交）；审计字段由 repo 统一盖章
    let result = dict_repo::update_dictionary(
        db,
        sys_dictionary::ActiveModel {
            id: Set(req.id),
            name: Set(req.name.clone()),
            r#type: Set(req.r#type.clone()),
            status: Set(req.status),
            remark: Set(req.remark.clone()),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(result)
}

/// 查询单个类型（排除软删）；不存在返回 Biz。
pub async fn get_dictionary(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<sys_dictionary::Model, AppError> {
    let Some(model) = dict_repo::find_dictionary_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("字典类型不存在：{}", id)));
    };

    Ok(model)
}

pub async fn delete_dictionary(
    db: &DatabaseConnection,
    id: u64,
    actor_id: u64,
) -> Result<u64, AppError> {
    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_dictionary_in_tx(&txn, id, actor_id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 删除类型：判存在 → 软删类型 → 级联软删其下字典项，返回字典项删除数量。
///
/// 返回级联删除的字典项行数（而非是否成功），供前端提示「已删除 N 项」；
/// 类型本体软删而非物理删，让 type 唯一键占位延续，避免历史数据引用悬空。
pub async fn delete_dictionary_in_tx(
    txn: &DatabaseTransaction,
    id: u64,
    actor_id: u64,
) -> Result<u64, AppError> {
    // 1) 目标必须存在（软删视为不存在）
    if dict_repo::find_dictionary_by_id(txn, id).await?.is_none() {
        return Err(AppError::Biz(format!("字典类型不存在：{}", id)));
    }

    // 2) 软删类型本体
    dict_repo::soft_delete_dictionary(txn, id, actor_id).await?;

    // 3) 级联软删其下全部活字典项，返回受影响行数
    let removed = dict_repo::soft_delete_details_by_dictionary_id(txn, id).await?;
    Ok(removed)
}

/// 按类型编码取「类型 + 启用字典项」（前端下拉用）。
///
/// 类型不存在 / 已软删 / `status != 1` 一律返回 Biz，文案统一为
/// `字典类型不存在或已禁用`；类型存在但无可用字典项时返回空 `details`，
/// 不报错（前端下拉显示空选项即可）。
pub async fn get_dictionary_by_type(
    db: &impl ConnectionTrait,
    r#type: &str,
) -> Result<(sys_dictionary::Model, Vec<sys_dictionary_detail::Model>), AppError> {
    // 1) 取未软删的类型（repo 已过滤 deleted_at）
    let Some(model) = dict_repo::find_dictionary_by_type(db, r#type).await? else {
        return Err(AppError::Biz(format!("字典类型不存在或已禁用：{}", r#type)));
    };
    // 2) 禁用类型不参与下拉
    if model.status != 1 {
        return Err(AppError::Biz(format!("字典类型不存在或已禁用：{}", r#type)));
    }

    // 3) 其下启用项按 sort 升序；空列表不报错
    let details = dict_repo::find_enabled_details(db, model.id).await?;

    Ok((model, details))
}

/// 取某字典类型启用项的**整数值**列表，供服务端字段值校验使用
/// （如 `type="status"` 返回 `[0, 1]`，对应 seed 中「禁用/启用」）。
///
/// 复用 `get_dictionary_by_type`：类型缺失/禁用会报错；字典项 value 需能解析为 `i8`。
pub async fn enabled_int_values(
    db: &impl ConnectionTrait,
    r#type: &str,
) -> Result<Vec<i8>, AppError> {
    let (_, details) = get_dictionary_by_type(db, r#type).await?;
    details
        .iter()
        .map(|d| {
            d.value.parse::<i8>().map_err(|_| {
                AppError::Biz(format!("字典 {} 的项值不是合法整数：{}", r#type, d.value))
            })
        })
        .collect()
}

// —— 字典项 ——

/// 字典项分页查询：请求参数（dictionary_id / keyword / status / 审计过滤）透传 repo。
pub async fn page_dictionary_details(
    db: &impl ConnectionTrait,
    req: &DictionaryDetailListReq,
) -> Result<PageData<sys_dictionary_detail::Model>, AppError> {
    let filter = DictionaryDetailFilter {
        dictionary_id: req.dictionary_id,
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
        dict_repo::find_detail_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 创建字典项：类型存在性校验 → 活记录 value 查重 → 落库。
///
/// value 唯一性口径：仅活记录占位，软删过的同 value 允许重建，
/// 因此查重必须用 `find_alive_detail_by_value`（过滤 deleted_at），
/// 不能依赖数据库唯一键（表上刻意不加）。
pub async fn create_dictionary_detail(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &CreateDictionaryDetailReq,
) -> Result<sys_dictionary_detail::Model, AppError> {
    // 1) 所属类型必须存在且未软删
    if dict_repo::find_dictionary_by_id(db, req.dictionary_id)
        .await?
        .is_none()
    {
        return Err(AppError::Biz(format!(
            "字典类型不存在：{}",
            req.dictionary_id
        )));
    }

    // 2) value 查重：仅活记录，软删的同 value 放行（可重建）
    if dict_repo::find_alive_detail_by_value(db, req.dictionary_id, &req.value)
        .await?
        .is_some()
    {
        return Err(AppError::Biz(format!("字典值已存在：{}", req.value)));
    }

    // 3) 落库（审计字段由 repo 统一盖章）
    let result = dict_repo::create_detail(
        db,
        sys_dictionary_detail::ActiveModel {
            dictionary_id: Set(req.dictionary_id),
            label: Set(req.label.clone()),
            value: Set(req.value.clone()),
            extend: Set(req.extend.clone()),
            sort: Set(req.sort),
            status: Set(req.status),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(result)
}

/// 更新字典项：判存在 → 类型校验 → value 查重排除自身 → 全量覆盖。
pub async fn update_dictionary_detail(
    db: &impl ConnectionTrait,
    actor_id: u64,
    req: &UpdateDictionaryDetailReq,
) -> Result<sys_dictionary_detail::Model, AppError> {
    // 1) 目标字典项必须存在（软删视为不存在）
    if dict_repo::find_detail_by_id(db, req.id).await?.is_none() {
        return Err(AppError::Biz(format!("字典项不存在：{}", req.id)));
    }
    // 2) 所属类型必须存在且未软删（防止把字典项挪到已删类型下）
    if dict_repo::find_dictionary_by_id(db, req.dictionary_id)
        .await?
        .is_none()
    {
        return Err(AppError::Biz(format!(
            "字典类型不存在：{}",
            req.dictionary_id
        )));
    }
    // 3) value 查重排除自身：保留自己的 value 合法，占用他人的才拒绝
    if let Some(existing) =
        dict_repo::find_alive_detail_by_value(db, req.dictionary_id, &req.value).await?
        && existing.id != req.id
    {
        return Err(AppError::Biz(format!("字典值已存在：{}", req.value)));
    }

    // 4) 全量覆盖更新（编辑表单整体提交；审计字段由 repo 统一盖章）
    let result = dict_repo::update_detail(
        db,
        sys_dictionary_detail::ActiveModel {
            id: Set(req.id),
            dictionary_id: Set(req.dictionary_id),
            label: Set(req.label.clone()),
            value: Set(req.value.clone()),
            extend: Set(req.extend.clone()),
            sort: Set(req.sort),
            status: Set(req.status),
            ..Default::default()
        },
        actor_id,
    )
    .await?;
    Ok(result)
}

/// 查询单个字典项（排除软删）；不存在返回 Biz。
pub async fn get_dictionary_detail(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<sys_dictionary_detail::Model, AppError> {
    let Some(model) = dict_repo::find_detail_by_id(db, id).await? else {
        return Err(AppError::Biz(format!("字典项不存在：{}", id)));
    };

    Ok(model)
}

/// 删除字典项：判存在后软删（软删行让出 value 唯一占位）。
pub async fn delete_dictionary_detail(db: &impl ConnectionTrait, id: u64) -> Result<(), AppError> {
    // 目标必须存在（软删视为不存在）
    if dict_repo::find_detail_by_id(db, id).await?.is_none() {
        return Err(AppError::Biz(format!("字典项不存在：{}", id)));
    }

    dict_repo::soft_delete_detail(db, id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::system::dictionary::dto::{
        CreateDictionaryDetailReq, CreateDictionaryReq, UpdateDictionaryReq,
    };
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    // 模块内自增序号。注意：repo.rs 测试也有同名同起点的独立 SEQ，
    // 若只拼 `<prefix>_<pid>_<seq>`，两个测试模块并行时会生成完全相同的
    // type/name 撞 `sys_dictionary.type` 唯一键（随机 flaky）。这里固定加
    // `svc_` 模块标记，使 service 测试数据与 repo 测试数据永不相交。
    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 既有测试不关心操作人，统一用种子 admin（id=1）作为 actor。
    const ACTOR_ID: u64 = 1;

    fn unique(prefix: &str) -> String {
        format!(
            "svc_{prefix}_{}_{}",
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

    fn now() -> chrono::NaiveDateTime {
        chrono::Local::now().naive_local()
    }

    async fn seed_dictionary(
        db: &impl ConnectionTrait,
        name: &str,
        r#type: &str,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_dictionary::Model {
        sys_dictionary::ActiveModel {
            name: Set(name.to_string()),
            r#type: Set(r#type.to_string()),
            status: Set(status),
            remark: Set(String::new()),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_detail(
        db: &impl ConnectionTrait,
        dictionary_id: u64,
        value: &str,
        label: &str,
        sort: i32,
        status: i8,
        deleted_at: Option<chrono::NaiveDateTime>,
    ) -> sys_dictionary_detail::Model {
        sys_dictionary_detail::ActiveModel {
            dictionary_id: Set(dictionary_id),
            label: Set(label.to_string()),
            value: Set(value.to_string()),
            extend: Set(String::new()),
            sort: Set(sort),
            status: Set(status),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    /// 测后清理：先物理删字典项，再删字典类型。
    ///
    /// 供自管事务的对外入口（`delete_dictionary`）测试使用——它内部 `begin()` 已提交，
    /// 外层事务无法回滚回收，只能真连接 + 手写清理。
    async fn cleanup(db: &impl ConnectionTrait, dictionary_ids: &[u64]) {
        sys_dictionary_detail::Entity::delete_many()
            .filter(
                sys_dictionary_detail::Column::DictionaryId.is_in(dictionary_ids.iter().copied()),
            )
            .exec(db)
            .await
            .unwrap();
        sys_dictionary::Entity::delete_many()
            .filter(sys_dictionary::Column::Id.is_in(dictionary_ids.iter().copied()))
            .exec(db)
            .await
            .unwrap();
    }

    fn create_req(r#type: String) -> CreateDictionaryReq {
        CreateDictionaryReq {
            name: unique("nm"),
            r#type,
            status: 1,
            remark: String::new(),
        }
    }

    fn update_req(id: u64, r#type: String) -> UpdateDictionaryReq {
        UpdateDictionaryReq {
            id,
            name: unique("nm"),
            r#type,
            status: 1,
            remark: String::new(),
        }
    }

    fn detail_req(dictionary_id: u64, value: &str) -> CreateDictionaryDetailReq {
        CreateDictionaryDetailReq {
            dictionary_id,
            label: unique("lb"),
            value: value.to_string(),
            extend: String::new(),
            sort: 0,
            status: 1,
        }
    }

    #[tokio::test]
    async fn create_dictionary_rejects_duplicate_type_including_soft_deleted() {
        let db = test_txn().await;
        let live = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let deleted = seed_dictionary(&db, &unique("nm"), &unique("tdel"), 1, Some(now())).await;

        let dup_live = create_dictionary(&db, ACTOR_ID, &create_req(live.r#type.clone())).await;
        let dup_deleted =
            create_dictionary(&db, ACTOR_ID, &create_req(deleted.r#type.clone())).await;

        assert!(
            matches!(dup_live, Err(AppError::Biz(_))),
            "活的 type 占位应拒绝重复，实际：{dup_live:?}"
        );
        assert!(
            matches!(dup_deleted, Err(AppError::Biz(_))),
            "软删 type 占位应拒绝重复，实际：{dup_deleted:?}"
        );
    }

    #[tokio::test]
    async fn update_dictionary_rejects_duplicate_type_excluding_self() {
        let db = test_txn().await;
        let a = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let b = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;

        let dup = update_dictionary(&db, ACTOR_ID, &update_req(b.id, a.r#type.clone())).await;
        let keep_self = update_dictionary(&db, ACTOR_ID, &update_req(b.id, b.r#type.clone())).await;

        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "占用他人 type 应拒绝，实际：{dup:?}"
        );
        keep_self.expect("保留自身 type 应更新成功");
    }

    /// 对外入口 `delete_dictionary`（自管事务）级联软删：类型与其下字典项一并失效。
    /// 走真实连接而非外层事务：自管事务无法被回滚隔离，故测后手写清理。
    #[tokio::test]
    async fn delete_dictionary_cascade_soft_deletes_its_details() {
        let db = test_db().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let _x = seed_detail(&db, d.id, &unique("v"), "lb", 1, 1, None).await;
        let _y = seed_detail(&db, d.id, &unique("v"), "lb", 2, 1, None).await;

        let removed = delete_dictionary(&db, d.id, ACTOR_ID).await.unwrap();
        let left = dict_repo::find_enabled_details(&db, d.id).await.unwrap();

        cleanup(&db, &[d.id]).await;

        assert_eq!(removed, 2, "应返回级联软删的字典项数量");
        assert!(left.is_empty(), "删类型后其下字典项不应再可见");
    }

    #[tokio::test]
    async fn create_detail_rejects_missing_type_and_duplicate_value() {
        let db = test_txn().await;

        let missing =
            create_dictionary_detail(&db, ACTOR_ID, &detail_req(9_999_999_999, "v")).await;
        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "类型不存在应拒绝，实际：{missing:?}"
        );

        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let first = create_dictionary_detail(&db, ACTOR_ID, &detail_req(d.id, "dup_value")).await;
        let dup = create_dictionary_detail(&db, ACTOR_ID, &detail_req(d.id, "dup_value")).await;

        first.expect("首建同 value 应成功");
        assert!(
            matches!(dup, Err(AppError::Biz(_))),
            "同类型活记录 value 重复应拒绝，实际：{dup:?}"
        );
    }

    #[tokio::test]
    async fn create_detail_allows_reusing_value_of_soft_deleted_row() {
        let db = test_txn().await;
        let d = seed_dictionary(&db, &unique("nm"), &unique("t"), 1, None).await;
        let old = seed_detail(&db, d.id, "dup_value", "lb", 0, 1, Some(now())).await;

        let fresh = create_dictionary_detail(&db, ACTOR_ID, &detail_req(d.id, "dup_value")).await;

        let fresh = fresh.expect("软删占位不应阻塞同 value 重建");
        assert_ne!(fresh.id, old.id, "应新建记录而非复用软删行");
    }

    #[tokio::test]
    async fn get_dictionary_by_type_returns_enabled_details_sorted() {
        let db = test_txn().await;
        let d = seed_dictionary(&db, "测试字典", &unique("t"), 1, None).await;
        let second = seed_detail(&db, d.id, &unique("v"), "lb", 2, 1, None).await;
        let first = seed_detail(&db, d.id, &unique("v"), "lb", 1, 1, None).await;
        let _off = seed_detail(&db, d.id, &unique("v"), "lb", 0, 0, None).await;
        let _deleted = seed_detail(&db, d.id, &unique("v"), "lb", 3, 1, Some(now())).await;

        let result = get_dictionary_by_type(&db, &d.r#type).await;

        let (model, details) = result.expect("启用类型应正常返回");
        assert_eq!(model.id, d.id);
        let ids: Vec<u64> = details.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![first.id, second.id], "只返回启用项且按 sort 升序");
    }

    #[tokio::test]
    async fn get_dictionary_by_type_errors_when_type_missing_or_disabled() {
        let db = test_txn().await;

        // 用唯一 type 保证「不存在」断言不受历史残留数据干扰
        let missing = get_dictionary_by_type(&db, &unique("miss")).await;
        let off = seed_dictionary(&db, &unique("nm"), &unique("t"), 0, None).await;
        let disabled = get_dictionary_by_type(&db, &off.r#type).await;

        assert!(
            matches!(missing, Err(AppError::Biz(_))),
            "不存在的 type 应返回 Biz，实际：{missing:?}"
        );
        assert!(
            matches!(disabled, Err(AppError::Biz(_))),
            "禁用类型应返回 Biz，实际：{disabled:?}"
        );
    }

    #[tokio::test]
    async fn get_update_and_delete_missing_return_biz_error() {
        let db = test_txn().await;

        let get_type = get_dictionary(&db, 9_999_999_999).await;
        let get_detail = get_dictionary_detail(&db, 9_999_999_999).await;
        let upd_type =
            update_dictionary(&db, ACTOR_ID, &update_req(9_999_999_999, unique("t"))).await;
        let del_detail = delete_dictionary_detail(&db, 9_999_999_999).await;

        assert!(
            matches!(get_type, Err(AppError::Biz(_))),
            "实际：{get_type:?}"
        );
        assert!(
            matches!(get_detail, Err(AppError::Biz(_))),
            "实际：{get_detail:?}"
        );
        assert!(
            matches!(upd_type, Err(AppError::Biz(_))),
            "实际：{upd_type:?}"
        );
        assert!(
            matches!(del_detail, Err(AppError::Biz(_))),
            "实际：{del_detail:?}"
        );
    }
}
