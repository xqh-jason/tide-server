//! 员工档案业务：分页 / 创建（查重 + 直属上级校验 + 可选联动建账号）/
//! 更新（敏感字段空串不改写）/ 查询 / 删除（软删）/ 批量取员工显示名。
//!
//! 写操作走 `*_in_tx` 业务实现 + 对外三行事务入口（同 position 域）；
//! 需要查库的规则（`user_id` 查重、存在性判定、直属上级合法性）在本层，
//! 值域校验在 `validate` 层。
//!
//! 「建档案联动建账号」必须整个落在**一个**事务里：账号建于同一 `txn`，
//! 因此只能调 `user_service::create_user_in_tx`，**不能**调 `user_service::create_user`
//! （后者自持 `db.begin()`；sea-orm 1.1.20 真库无 savepoint，事务内嵌套 begin 会被
//! MySQL 隐式提交，原子性即失效）。

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction};

use crate::entity::hr_employee;
use crate::modules::biz::hr::employee::dto::{
    CreateEmployeeReq, EmployeeFilter, EmployeeListReq, UpdateEmployeeReq,
};
use crate::utils::PageData;
use crate::utils::error::AppError;

/// 解析 `yyyy-MM-dd` 日期入参；`None` / 空串视为未设置。
///
/// 格式错误报「{field}格式应为 yyyy-MM-dd」。
fn parse_date(field: &str, v: &Option<String>) -> Result<Option<chrono::NaiveDate>, AppError> {
    let Some(raw) = v.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| AppError::Biz(format!("{field}格式应为 yyyy-MM-dd")))
}

/// 批量取员工显示名：`hr_employee.id` → `sys_user.username`。
///
/// 一次批量取档案（拿 `user_id`；软删档案不出现在映射里）→ 一次批量查名
/// （`utils::user_ref::find_user_name_map_by_ids`，全项目唯一查名管道）→
/// 回填 `employee_id → 显示名`。空入参早返；**不得逐行查库**。
///
/// **全仓唯一实现**：time_off / attendance / overtime 三个域的显示名拼装都走本函数
/// （各自的同构副本已删除），不得再新增第二份；查不到的人（软删 / 不存在）不出现，
/// 调用方留空串。
pub async fn find_employee_name_map(
    db: &impl ConnectionTrait,
    ids: &[u64],
) -> Result<HashMap<u64, String>, AppError> {
    use crate::modules::biz::hr::employee::repo as employee_repo;
    use crate::utils::user_ref::{dedup_ids, find_user_name_map_by_ids};

    let ids = dedup_ids(ids.to_vec());
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let employees = employee_repo::find_by_ids(db, &ids).await?;
    let user_ids = employees.iter().map(|e| e.user_id).collect();
    let names = find_user_name_map_by_ids(db, user_ids).await?;
    Ok(employees
        .into_iter()
        .filter_map(|e| names.get(&e.user_id).cloned().map(|name| (e.id, name)))
        .collect())
}

/// 档案分页：请求参数（keyword / 状态 / 学历 / 审计过滤）组装为 repo 过滤条件。
///
/// 纯透传，不做业务判断；四个时间字段交 `utils::datetime::parse_datetime`
/// （范围起 `false`、范围止 `true`）。
pub async fn page_employees(
    db: &impl ConnectionTrait,
    req: &EmployeeListReq,
) -> Result<PageData<hr_employee::Model>, AppError> {
    use crate::modules::biz::hr::employee::repo as employee_repo;
    use crate::utils::datetime::parse_datetime;

    let filter = EmployeeFilter {
        keyword: req.keyword.clone(),
        employment_status: req.employment_status,
        education: req.education,
        created_by: req.created_by,
        updated_by: req.updated_by,
        manager_employee_id: req.manager_employee_id,
        created_at_begin: parse_datetime("createdAtBegin", &req.created_at_begin, false)?,
        created_at_end: parse_datetime("createdAtEnd", &req.created_at_end, true)?,
        updated_at_begin: parse_datetime("updatedAtBegin", &req.updated_at_begin, false)?,
        updated_at_end: parse_datetime("updatedAtEnd", &req.updated_at_end, true)?,
    };

    Ok(
        employee_repo::find_employee_page(db, &filter, req.page.page_index(), req.page.page_size())
            .await?,
    )
}

/// 事务内创建档案：解析账号来源（关联已有 / 同事务新建）→ `user_id` 查重 →
/// 直属上级存在性校验 → 落库。
///
/// `user_id` 与 `create_account` 恰好二选一（都传 / 都不传都是业务错误）。
/// `manager_employee_id` 为 `0` 表示未设置，非 0 时必须指向有效档案（软删视为不存在）。
pub(crate) async fn create_employee_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: CreateEmployeeReq,
) -> Result<hr_employee::Model, AppError> {
    use crate::modules::biz::hr::employee::repo as employee_repo;
    use crate::modules::system::user::dto::CreateUserReq;
    use crate::modules::system::user::service as user_service;
    use sea_orm::ActiveValue::Set;

    // 账号来源：关联已有账号（校验存在，软删即不存在）/ 同事务新建账号
    let user_id = match (&req.user_id, &req.create_account) {
        (Some(_), Some(_)) => {
            return Err(AppError::Biz("userId 与 createAccount 只能二选一".into()));
        }
        (None, None) => {
            return Err(AppError::Biz("必须指定 userId 或 createAccount".into()));
        }
        (Some(id), None) => {
            user_service::get_user(txn, *id).await?;
            *id
        }
        (None, Some(acc)) => {
            let user = user_service::create_user_in_tx(
                txn,
                actor_id,
                CreateUserReq {
                    username: acc.username.clone(),
                    password: acc.password.clone(),
                    emp_no: acc.emp_no.clone(),
                    nickname: acc.nickname.clone(),
                    phone: acc.phone.clone(),
                    email: acc.email.clone(),
                    status: 1,
                    role_ids: acc.role_ids.clone(),
                    depts: vec![],
                    position_ids: vec![],
                },
            )
            .await?;
            user.id
        }
    };

    // user_id 是单列唯一键且软删行仍占位：查重必须含软删记录
    if employee_repo::find_by_user_id_include_deleted(txn, user_id)
        .await?
        .is_some()
    {
        return Err(AppError::Biz("该用户已有员工档案".into()));
    }

    // 直属上级必须是有员工的档案（软删视为不存在）；0 = 未设置。
    // 新建档案的 id 尚未生成，因此不存在「把自己设为上级」的可能，无需比对。
    if req.manager_employee_id != 0
        && employee_repo::find_by_id(txn, req.manager_employee_id)
            .await?
            .is_none()
    {
        return Err(AppError::Biz(format!(
            "直属上级不存在：{}",
            req.manager_employee_id
        )));
    }

    // 审计字段由 repo 统一盖章，入参不含人字段
    let model = hr_employee::ActiveModel {
        user_id: Set(user_id),
        manager_employee_id: Set(req.manager_employee_id),
        hire_date: Set(parse_date("入职日期", &req.hire_date)?),
        regular_date: Set(parse_date("转正日期", &req.regular_date)?),
        leave_date: Set(parse_date("离职日期", &req.leave_date)?),
        employment_status: Set(req.employment_status),
        education: Set(req.education),
        graduate_school: Set(req.graduate_school.clone()),
        major: Set(req.major.clone()),
        id_card: Set(req.id_card.clone()),
        emergency_contact: Set(req.emergency_contact.clone()),
        emergency_phone: Set(req.emergency_phone.clone()),
        bank_account: Set(req.bank_account.clone()),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };

    Ok(employee_repo::create_employee_in_tx(txn, model, actor_id).await?)
}

/// 事务内更新档案：判存在 → 直属上级三态校验（缺省保留 / 0 清空 / 非本人且存在）→
/// 敏感字段空串保持原值 → 窄写（不动 `user_id`）。
pub(crate) async fn update_employee_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    req: &UpdateEmployeeReq,
) -> Result<hr_employee::Model, AppError> {
    use crate::modules::biz::hr::employee::repo as employee_repo;
    use sea_orm::ActiveValue::{NotSet, Set};

    let Some(existing) = employee_repo::find_by_id(txn, req.id).await? else {
        return Err(AppError::Biz(format!("员工档案不存在：{}", req.id)));
    };

    // 直属上级三态：缺省 / `null` = 不修改（写 NotSet 保留原值）；0 = 清空；
    // >0 = 改写为该员工，且不能是本人、必须存在（软删视为不存在）
    let manager_employee_id = match req.manager_employee_id {
        None => NotSet,
        Some(0) => Set(0),
        Some(id) => {
            if id == req.id {
                return Err(AppError::Biz("直属上级不能是本人".into()));
            }
            if employee_repo::find_by_id(txn, id).await?.is_none() {
                return Err(AppError::Biz(format!("直属上级不存在：{id}")));
            }
            Set(id)
        }
    };

    // 敏感字段空串 = 不修改（列表 / 详情回传掩码值，前端编辑表单不回填）
    let id_card = if req.id_card.trim().is_empty() {
        existing.id_card
    } else {
        req.id_card.clone()
    };
    let bank_account = if req.bank_account.trim().is_empty() {
        existing.bank_account
    } else {
        req.bank_account.clone()
    };

    // 窄写：只 Set 业务变更列（不动 user_id，`created_by` 由 repo 保持 NotSet）
    let model = hr_employee::ActiveModel {
        id: Set(req.id),
        manager_employee_id,
        hire_date: Set(parse_date("入职日期", &req.hire_date)?),
        regular_date: Set(parse_date("转正日期", &req.regular_date)?),
        leave_date: Set(parse_date("离职日期", &req.leave_date)?),
        employment_status: Set(req.employment_status),
        education: Set(req.education),
        graduate_school: Set(req.graduate_school.clone()),
        major: Set(req.major.clone()),
        id_card: Set(id_card),
        emergency_contact: Set(req.emergency_contact.clone()),
        emergency_phone: Set(req.emergency_phone.clone()),
        bank_account: Set(bank_account),
        remark: Set(req.remark.clone()),
        ..Default::default()
    };

    Ok(employee_repo::update_employee_in_tx(txn, model, actor_id).await?)
}

/// 事务内删除档案：判存在 → 软删（关联登录账号保留，由 user 域单独管理）。
pub(crate) async fn delete_employee_in_tx(
    txn: &DatabaseTransaction,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    use crate::modules::biz::hr::employee::repo as employee_repo;

    if employee_repo::find_by_id(txn, id).await?.is_none() {
        return Err(AppError::Biz(format!("员工档案不存在：{id}")));
    }
    // false = 并发下已被他人删掉，同样按「不存在」处理
    if !employee_repo::soft_delete_employee_in_tx(txn, id, actor_id).await? {
        return Err(AppError::Biz(format!("员工档案不存在：{id}")));
    }
    Ok(())
}

/// 按 id 查档案详情（软删视为不存在）。
pub async fn get_employee(
    db: &impl ConnectionTrait,
    id: u64,
) -> Result<hr_employee::Model, AppError> {
    use crate::modules::biz::hr::employee::repo as employee_repo;

    employee_repo::find_by_id(db, id)
        .await?
        .ok_or_else(|| AppError::Biz(format!("员工档案不存在：{id}")))
}

/// 对外入口：开事务后委托 `create_employee_in_tx`，成功后提交。
pub async fn create_employee(
    db: &DatabaseConnection,
    actor_id: u64,
    req: CreateEmployeeReq,
) -> Result<hr_employee::Model, AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = create_employee_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 对外入口：开事务后委托 `update_employee_in_tx`，成功后提交。
pub async fn update_employee(
    db: &DatabaseConnection,
    actor_id: u64,
    req: &UpdateEmployeeReq,
) -> Result<hr_employee::Model, AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = update_employee_in_tx(&txn, actor_id, req).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

/// 对外入口：开事务后委托 `delete_employee_in_tx`，成功后提交。
pub async fn delete_employee(
    db: &DatabaseConnection,
    actor_id: u64,
    id: u64,
) -> Result<(), AppError> {
    use sea_orm::TransactionTrait;

    let txn = db.begin().await.map_err(anyhow::Error::from)?;
    let result = delete_employee_in_tx(&txn, actor_id, id).await;
    if result.is_ok() {
        txn.commit().await.map_err(anyhow::Error::from)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::biz::hr::employee::dto::CreateAccountReq;
    use crate::modules::system::user::service as user_service;
    use sea_orm::{ActiveModelTrait, Database, Set};
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

    /// 造一个真实平台账号（走 entity 直插，只借 user 表做「已有账号」）。
    async fn seed_user(db: &impl ConnectionTrait) -> crate::entity::sys_user::Model {
        crate::entity::sys_user::ActiveModel {
            username: Set(unique("hr_user")),
            password: Set("x".to_string()),
            nickname: Set("员工测试账号".to_string()),
            status: Set(1),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn base_req(
        user_id: Option<u64>,
        create_account: Option<CreateAccountReq>,
    ) -> CreateEmployeeReq {
        CreateEmployeeReq {
            user_id,
            create_account,
            manager_employee_id: 0,
            hire_date: Some("2026-01-01".to_string()),
            regular_date: None,
            leave_date: None,
            employment_status: 1,
            education: 3,
            graduate_school: String::new(),
            major: String::new(),
            id_card: String::new(),
            emergency_contact: String::new(),
            emergency_phone: String::new(),
            bank_account: String::new(),
            remark: String::new(),
        }
    }

    fn account_req() -> CreateAccountReq {
        CreateAccountReq {
            username: unique("hr_acc"),
            password: "Passw0rd!23".to_string(),
            nickname: "新入职员工".to_string(),
            emp_no: String::new(),
            phone: String::new(),
            email: String::new(),
            role_ids: vec![],
        }
    }

    /// 只改直属上级的更新请求骨架：敏感字段空串 = 不改写，其余业务列取稳定值。
    /// 上级三态：`None` = 不修改、`Some(0)` = 清空、`Some(id)` = 设为该员工。
    fn update_req(id: u64, manager_employee_id: Option<u64>) -> UpdateEmployeeReq {
        UpdateEmployeeReq {
            id,
            manager_employee_id,
            hire_date: Some("2026-01-01".to_string()),
            regular_date: None,
            leave_date: None,
            employment_status: 1,
            education: 3,
            graduate_school: String::new(),
            major: String::new(),
            id_card: String::new(),
            emergency_contact: String::new(),
            emergency_phone: String::new(),
            bank_account: String::new(),
            remark: String::new(),
        }
    }

    #[tokio::test]
    async fn create_employee_with_existing_user_links_it() {
        let db = test_txn().await;
        let user = seed_user(&db).await;

        let created = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None))
            .await
            .unwrap();

        assert_eq!(created.user_id, user.id, "应落到请求指定的账号");
        assert_eq!(created.created_by, ACTOR_ID, "创建人应为操作人");
        assert_eq!(created.updated_by, ACTOR_ID, "创建时更新人与创建人同源");
    }

    #[tokio::test]
    async fn create_employee_rejects_duplicate_profile_for_same_user() {
        let db = test_txn().await;
        let user = seed_user(&db).await;

        let first = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None)).await;
        let second = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None)).await;

        assert!(first.is_ok(), "首个档案应创建成功");
        let err = second.unwrap_err();
        assert!(
            err.to_string().contains("已有员工档案"),
            "同账号第二份档案应被拒，实际：{err}"
        );
    }

    #[tokio::test]
    async fn create_employee_rejects_duplicate_profile_for_soft_deleted_user() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let created = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None))
            .await
            .unwrap();
        delete_employee_in_tx(&db, ACTOR_ID, created.id)
            .await
            .unwrap();

        let again = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None)).await;

        let err = again.unwrap_err();
        assert!(
            err.to_string().contains("已有员工档案"),
            "user_id 唯一键含软删占位，软删后同账号仍不许重建，实际：{err}"
        );
    }

    #[tokio::test]
    async fn create_employee_with_account_creates_user_in_same_txn() {
        let db = test_txn().await;
        let account = account_req();
        let username = account.username.clone();

        let created = create_employee_in_tx(&db, ACTOR_ID, base_req(None, Some(account)))
            .await
            .unwrap();

        let user = user_service::get_user(&db, created.user_id).await.unwrap();
        assert_eq!(user.username, username, "档案应关联到新建账号");
        assert!(
            user.password.starts_with("$argon2id$"),
            "新账号密码必须是 Argon2id 哈希，实际：{}",
            user.password
        );
        assert_eq!(user.status, 1, "新建账号默认启用");
    }

    #[tokio::test]
    async fn create_employee_requires_exactly_one_of_user_id_or_account() {
        let db = test_txn().await;
        let user = seed_user(&db).await;

        let neither = create_employee_in_tx(&db, ACTOR_ID, base_req(None, None)).await;
        let both =
            create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), Some(account_req())))
                .await;

        let neither_err = neither.unwrap_err();
        assert!(
            neither_err.to_string().contains("userId 或 createAccount"),
            "都不传应提示必须指定账号来源，实际：{neither_err}"
        );
        let both_err = both.unwrap_err();
        assert!(
            both_err.to_string().contains("只能二选一"),
            "都传应提示二选一，实际：{both_err}"
        );
    }

    /// 未传 `managerEmployeeId` 的部分更新不得清空直属上级：档案的上级是审批链「直属上级」
    /// 节点的唯一来源，静默清空会让该员工的加班单无法提交、请假单少一级审批。
    #[tokio::test]
    async fn update_without_manager_field_keeps_existing_manager() {
        let db = test_txn().await;
        let manager_user = seed_user(&db).await;
        let user = seed_user(&db).await;
        let manager = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(manager_user.id), None))
            .await
            .unwrap();
        let created = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None))
            .await
            .unwrap();

        // 先挂上直属上级
        let linked =
            update_employee_in_tx(&db, ACTOR_ID, &update_req(created.id, Some(manager.id)))
                .await
                .unwrap();
        assert_eq!(linked.manager_employee_id, manager.id, "前置：上级已挂上");

        // 前端部分更新（不带 managerEmployeeId）→ 上级必须保持不变
        let partial: UpdateEmployeeReq = serde_json::from_value(serde_json::json!({
            "id": created.id,
            "hireDate": "2026-03-03",
            "employmentStatus": 1,
            "education": 3
        }))
        .unwrap();
        let updated = update_employee_in_tx(&db, ACTOR_ID, &partial)
            .await
            .unwrap();
        assert_eq!(
            updated.manager_employee_id, manager.id,
            "缺省字段不得清空直属上级（显式传 0 才是清空）"
        );

        // 显式传 0 = 清空上级
        let cleared = update_employee_in_tx(&db, ACTOR_ID, &update_req(created.id, Some(0)))
            .await
            .unwrap();
        assert_eq!(cleared.manager_employee_id, 0, "显式 0 应清空上级");
    }

    #[tokio::test]
    async fn update_employee_keeps_sensitive_when_blank() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let mut req = base_req(Some(user.id), None);
        req.id_card = "110101199003071234".to_string();
        req.bank_account = "6222021234567890123".to_string();
        let created = create_employee_in_tx(&db, ACTOR_ID, req).await.unwrap();

        let updated = update_employee_in_tx(
            &db,
            ACTOR_ID,
            &UpdateEmployeeReq {
                id: created.id,
                manager_employee_id: Some(0),
                hire_date: Some("2026-02-02".to_string()),
                regular_date: Some("2026-05-02".to_string()),
                leave_date: None,
                employment_status: 2,
                education: 4,
                graduate_school: "测试大学".to_string(),
                major: "软件工程".to_string(),
                // 敏感字段空串 = 不修改（前端表单不回填掩码值）
                id_card: String::new(),
                emergency_contact: "紧急联系人".to_string(),
                emergency_phone: "13800000000".to_string(),
                bank_account: String::new(),
                remark: "转正".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.id_card, created.id_card, "敏感字段空串不得改写原值");
        assert_eq!(
            updated.bank_account, created.bank_account,
            "敏感字段空串不得改写原值"
        );
        assert_eq!(updated.employment_status, 2, "非敏感字段应正常更新");
        assert_eq!(
            updated.hire_date.map(|d| d.to_string()),
            Some("2026-02-02".to_string()),
            "日期应按 yyyy-MM-dd 落库"
        );
        assert_eq!(updated.updated_by, ACTOR_ID, "更新人应盖章为操作人");
        assert_eq!(updated.created_by, created.created_by, "创建人不被覆盖");
    }

    #[tokio::test]
    async fn delete_employee_soft_deletes_and_keeps_account() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let created = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None))
            .await
            .unwrap();

        delete_employee_in_tx(&db, ACTOR_ID, created.id)
            .await
            .unwrap();

        assert!(
            crate::modules::biz::hr::employee::repo::find_by_id(&db, created.id)
                .await
                .unwrap()
                .is_none(),
            "软删后 find_by_id 应查不到"
        );
        assert!(
            crate::modules::biz::hr::employee::repo::find_by_user_id_include_deleted(&db, user.id)
                .await
                .unwrap()
                .is_some(),
            "软删档案仍占位"
        );
        assert!(
            user_service::get_user(&db, user.id).await.is_ok(),
            "删档案不动关联账号"
        );
        let missing = delete_employee_in_tx(&db, ACTOR_ID, 9_999_999_999).await;
        let missing_err = missing.unwrap_err();
        assert!(
            missing_err.to_string().contains("员工档案不存在"),
            "不存在的档案应按「不存在」处理，实际：{missing_err}"
        );
    }

    #[tokio::test]
    async fn create_employee_rejects_manager_without_live_profile() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let mut req = base_req(Some(user.id), None);
        req.manager_employee_id = 9_999_999_999;

        let err = create_employee_in_tx(&db, ACTOR_ID, req).await.unwrap_err();

        assert!(
            err.to_string().contains("直属上级不存在"),
            "不存在的上级应被拒，实际：{err}"
        );
    }

    #[tokio::test]
    async fn create_employee_rejects_soft_deleted_manager() {
        let db = test_txn().await;
        let manager_user = seed_user(&db).await;
        let manager = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(manager_user.id), None))
            .await
            .unwrap();
        delete_employee_in_tx(&db, ACTOR_ID, manager.id)
            .await
            .unwrap();

        let user = seed_user(&db).await;
        let mut req = base_req(Some(user.id), None);
        req.manager_employee_id = manager.id;
        let err = create_employee_in_tx(&db, ACTOR_ID, req).await.unwrap_err();

        assert!(
            err.to_string().contains("直属上级不存在"),
            "软删档案不算有效上级，实际：{err}"
        );
    }

    #[tokio::test]
    async fn update_employee_rejects_self_as_manager() {
        let db = test_txn().await;
        let user = seed_user(&db).await;
        let created = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(user.id), None))
            .await
            .unwrap();

        let err = update_employee_in_tx(&db, ACTOR_ID, &update_req(created.id, Some(created.id)))
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains("直属上级不能是本人"),
            "不能把自己设成自己的上级，实际：{err}"
        );
    }

    /// 直属上级三态：创建落值 / 更新改写 / 更新传 0 清空。
    #[tokio::test]
    async fn create_and_update_employee_store_manager_employee_id() {
        let db = test_txn().await;
        let first_manager_user = seed_user(&db).await;
        let first_manager =
            create_employee_in_tx(&db, ACTOR_ID, base_req(Some(first_manager_user.id), None))
                .await
                .unwrap();
        let second_manager_user = seed_user(&db).await;
        let second_manager =
            create_employee_in_tx(&db, ACTOR_ID, base_req(Some(second_manager_user.id), None))
                .await
                .unwrap();

        let user = seed_user(&db).await;
        let mut req = base_req(Some(user.id), None);
        req.manager_employee_id = first_manager.id;
        let created = create_employee_in_tx(&db, ACTOR_ID, req).await.unwrap();
        assert_eq!(
            created.manager_employee_id, first_manager.id,
            "创建应落直属上级"
        );

        let moved = update_employee_in_tx(
            &db,
            ACTOR_ID,
            &update_req(created.id, Some(second_manager.id)),
        )
        .await
        .unwrap();
        assert_eq!(
            moved.manager_employee_id, second_manager.id,
            "更新应改写直属上级"
        );
        let reread = get_employee(&db, created.id).await.unwrap();
        assert_eq!(
            reread.manager_employee_id, second_manager.id,
            "改写后的上级应已落库"
        );

        let cleared = update_employee_in_tx(&db, ACTOR_ID, &update_req(created.id, Some(0)))
            .await
            .unwrap();
        assert_eq!(cleared.manager_employee_id, 0, "更新传 0 应清空直属上级");
    }

    /// 批量取名：空入参早返；命中回填显示名；软删 / 不存在的员工缺席。
    #[tokio::test]
    async fn find_employee_name_map_maps_live_employees_only() {
        let db = test_txn().await;
        let live_user = seed_user(&db).await;
        let live = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(live_user.id), None))
            .await
            .unwrap();
        let deleted_user = seed_user(&db).await;
        let deleted = create_employee_in_tx(&db, ACTOR_ID, base_req(Some(deleted_user.id), None))
            .await
            .unwrap();
        delete_employee_in_tx(&db, ACTOR_ID, deleted.id)
            .await
            .unwrap();

        let empty = find_employee_name_map(&db, &[]).await.unwrap();
        assert!(empty.is_empty(), "空入参应直接返回空映射");

        let names = find_employee_name_map(&db, &[live.id, deleted.id, 9_999_999_999])
            .await
            .unwrap();
        assert_eq!(
            names.get(&live.id).map(String::as_str),
            Some(live_user.username.as_str()),
            "应回填为档案所属账号的显示名"
        );
        assert!(!names.contains_key(&deleted.id), "软删档案不应出现在映射里");
        assert!(
            !names.contains_key(&9_999_999_999),
            "不存在的员工不应出现在映射里"
        );
    }
}
