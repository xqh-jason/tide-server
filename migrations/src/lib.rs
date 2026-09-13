pub use sea_orm_migration::prelude::*;

mod legacy;
mod m20260913_000001_baseline;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(legacy::m20260825_000001_create_rbac_tables::Migration),
            Box::new(legacy::m20260827_000002_add_rbac_column_comments::Migration),
            Box::new(legacy::m20260827_000003_add_rbac_table_comments::Migration),
            Box::new(legacy::m20260827_000004_restore_menu_parent_default::Migration),
            Box::new(legacy::m20260901_000005_create_sys_dict::Migration),
            Box::new(legacy::m20260902_000006_add_user_emp_no::Migration),
            Box::new(legacy::m20260903_000007_create_sys_operation_log::Migration),
            Box::new(legacy::m20260903_000008_create_sys_login_log::Migration),
            Box::new(legacy::m20260903_000009_create_sys_dictionary::Migration),
            Box::new(legacy::m20260904_000010_add_audit_columns::Migration),
            Box::new(legacy::m20260905_000011_create_sys_file::Migration),
            Box::new(legacy::m20260905_000012_create_sys_config_and_sys_site_config::Migration),
            Box::new(legacy::m20260907_000013_create_sys_job_and_sys_job_log::Migration),
            Box::new(legacy::m20260908_000014_add_role_api_api_index::Migration),
            Box::new(legacy::m20260908_000015_add_role_menu_menu_index::Migration),
            Box::new(legacy::m20260908_000016_normalize_index_names::Migration),
            Box::new(legacy::m20260909_000017_add_missing_column_table_comments::Migration),
            Box::new(legacy::m20260909_000018_create_sys_dept::Migration),
            Box::new(legacy::m20260909_000019_create_sys_user_dept::Migration),
            Box::new(legacy::m20260909_000020_drop_sys_dept_leader_phone_email::Migration),
            Box::new(legacy::m20260909_000021_add_user_dept_primary_unique::Migration),
            Box::new(legacy::m20260909_000022_add_sys_menu_name_unique::Migration),
            Box::new(legacy::m20260909_000023_widen_sys_dept_path::Migration),
            Box::new(legacy::m20260909_000024_create_sys_position::Migration),
            Box::new(legacy::m20260909_000025_create_sys_user_position::Migration),
            // 基线迁移必须排在占位之后：新库先空跑占位，再由 baseline 一次性建表
            Box::new(m20260913_000001_baseline::Migration),
        ]
    }
}
