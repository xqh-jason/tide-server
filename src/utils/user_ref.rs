//! 用户引用字段名称拼装：created_by / updated_by 等指向 sys_user 的人字段，
//! 统一在此补齐 `*_name`，前端不做 id → 名称换算。
//!
//! 约定（AGENTS.md「人字段命名与名称拼装约定」）：
//! - 实体实现 `UserRefIds`：收集本记录全部人字段 id；
//! - Resp 实现 `UserRefNames`：按名称映射填充人名字段（查不到给空串）；
//! - `fill_user_names` 是全项目唯一拼装管道：收集 → 一次批量查 → 填充。
//!

use crate::entity::{
    hr_approval_flow, hr_approval_instance, hr_approval_record, hr_attendance_record, hr_employee,
    hr_overtime_request, hr_shift, hr_shift_schedule, hr_time_off_balance, hr_time_off_balance_log,
    hr_time_off_grant, hr_time_off_request, hr_time_off_type, hr_work_calendar, sys_api,
    sys_config, sys_dictionary, sys_dictionary_detail, sys_file, sys_job, sys_menu,
    sys_operation_log, sys_position, sys_refresh_token, sys_role, sys_site_config, sys_user,
};
use std::collections::HashMap;

use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};

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
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
    items: Vec<M>,
    convert: impl Fn(M) -> R,
) -> Result<Vec<R>, AppError> {
    let ids: Vec<u64> = items.iter().flat_map(|m| m.user_ref_ids()).collect();
    let names = find_user_name_map_by_ids(db, ids).await?;
    Ok(items
        .into_iter()
        .map(|m| {
            let mut resp = convert(m);
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

impl UserRefIds for sys_file::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_config::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_site_config::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_job::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for sys_position::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_employee::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        // 审计人字段 + 关联账号（档案所属员工），一次批量查三类显示名
        vec![self.created_by, self.updated_by, self.user_id]
    }
}

impl UserRefIds for sys_operation_log::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.user_id]
    }
}

impl UserRefIds for sys_refresh_token::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.revoked_by]
    }
}

impl UserRefIds for hr_approval_flow::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_approval_instance::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        // 申请人 + 当前审批人（角色池时为 0，取名时空缺）+ 审计人字段
        vec![
            self.applicant_id,
            self.current_approver_id,
            self.created_by,
            self.updated_by,
        ]
    }
}

impl UserRefIds for hr_approval_record::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        // 审批人（角色池时为 0）+ 实际操作人
        vec![self.approver_id, self.acted_by]
    }
}

impl UserRefIds for hr_time_off_type::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_time_off_grant::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_time_off_balance::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_time_off_balance_log::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        // append-only 流水表：无 created_by / updated_by 审计人字段对，唯一人字段是操作人
        vec![self.operator_id]
    }
}

impl UserRefIds for hr_time_off_request::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_overtime_request::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_shift::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_shift_schedule::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_attendance_record::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

impl UserRefIds for hr_work_calendar::Model {
    fn user_ref_ids(&self) -> Vec<u64> {
        vec![self.created_by, self.updated_by]
    }
}

#[cfg(test)]
mod tests {

    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
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

    /// 事务连接：测试结束（含 panic 时 Drop）自动 ROLLBACK，不留孤儿数据。
    async fn test_txn() -> sea_orm::DatabaseTransaction {
        use sea_orm::TransactionTrait;
        test_db().await.begin().await.unwrap()
    }

    /// 造一个测试用户，返回 (id, username)。
    async fn seed_user(db: &impl ConnectionTrait) -> (u64, String) {
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
    async fn seed_deleted_user(db: &impl ConnectionTrait) -> (u64, String) {
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
        let db = test_txn().await;
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
        let db = test_txn().await;
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
        let db = test_txn().await;
        let map = find_user_name_map_by_ids(&db, vec![]).await.unwrap();
        assert!(map.is_empty(), "空入参不应发查询");
    }

    #[tokio::test]
    async fn fill_user_names_fills_all_records_in_one_batch() {
        let db = test_txn().await;
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
        let db = test_txn().await;
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

    /// HR 员工档案：人字段 id 收集应含创建人 / 更新人 / 关联账号三类。
    #[test]
    fn hr_employee_collects_audit_and_account_ids() {
        use crate::entity::hr_employee;
        let row = hr_employee::Model {
            id: 1,
            user_id: 7,
            manager_employee_id: 0,
            hire_date: None,
            regular_date: None,
            leave_date: None,
            employment_status: 1,
            education: 0,
            graduate_school: String::new(),
            major: String::new(),
            id_card: String::new(),
            emergency_contact: String::new(),
            emergency_phone: String::new(),
            bank_account: String::new(),
            remark: String::new(),
            created_at: chrono::Local::now().naive_local(),
            updated_at: chrono::Local::now().naive_local(),
            created_by: 3,
            updated_by: 4,
            deleted_at: None,
        };
        let mut ids = row.user_ref_ids();
        ids.sort_unstable();
        assert_eq!(ids, vec![3, 4, 7], "应收集创建人/更新人/关联账号三类 id");
    }

    /// HR 假期额度账本：批次收集审计人字段对；append-only 流水只收集操作人。
    #[test]
    fn user_ref_ids_covers_time_off_ledger_entities() {
        let grant = hr_time_off_grant::Model {
            id: 1,
            employee_id: 2,
            time_off_type_id: 3,
            source: 2,
            reason: "manual".to_string(),
            source_kind: 0,
            source_id: 0,
            period: "2026".to_string(),
            minutes: 480,
            remaining_minutes: 240,
            effective_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            expire_at: None,
            status: 1,
            remark: String::new(),
            created_at: chrono::Local::now().naive_local(),
            updated_at: chrono::Local::now().naive_local(),
            created_by: 5,
            updated_by: 6,
        };
        assert_eq!(
            grant.user_ref_ids(),
            vec![5, 6],
            "批次应收集创建人 / 更新人 id"
        );

        let log = hr_time_off_balance_log::Model {
            id: 1,
            balance_id: 2,
            employee_id: 3,
            time_off_type_id: 4,
            grant_id: 5,
            biz_type: 1,
            delta_minutes: 480,
            before_minutes: 0,
            after_minutes: 480,
            source_kind: 0,
            source_id: 0,
            operator_id: 7,
            remark: String::new(),
            created_at: chrono::Local::now().naive_local(),
        };
        assert_eq!(
            log.user_ref_ids(),
            vec![7],
            "流水应只收集操作人 id（无审计人字段对）"
        );
    }

    /// HR 假期额度：假期类型与额度账户都只有审计人字段对（`created_by` / `updated_by`）。
    #[test]
    fn user_ref_ids_covers_time_off_type_and_balance() {
        let time_off_type = hr_time_off_type::Model {
            id: 1,
            type_code: "annual".to_string(),
            type_name: "年假".to_string(),
            unit: 1,
            balance_mode: 1,
            min_unit_minutes: 240,
            require_attachment: 0,
            allow_negative: 0,
            pay_ratio: 1000,
            status: 1,
            remark: String::new(),
            created_at: chrono::Local::now().naive_local(),
            updated_at: chrono::Local::now().naive_local(),
            created_by: 8,
            updated_by: 9,
            deleted_at: None,
        };
        assert_eq!(
            time_off_type.user_ref_ids(),
            vec![8, 9],
            "假期类型应收集创建人 / 更新人 id"
        );

        let balance = hr_time_off_balance::Model {
            id: 1,
            employee_id: 2,
            time_off_type_id: 3,
            period: "2026".to_string(),
            granted_minutes: 4800,
            used_minutes: 480,
            locked_minutes: 0,
            expired_minutes: 0,
            adjust_minutes: 0,
            created_at: chrono::Local::now().naive_local(),
            updated_at: chrono::Local::now().naive_local(),
            created_by: 10,
            updated_by: 11,
        };
        assert_eq!(
            balance.user_ref_ids(),
            vec![10, 11],
            "额度账户应收集创建人 / 更新人 id"
        );
    }
}
