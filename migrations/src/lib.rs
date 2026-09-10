pub use sea_orm_migration::prelude::*;

mod m20260825_000001_create_rbac_tables;
mod m20260827_000002_add_rbac_column_comments;
mod m20260827_000003_add_rbac_table_comments;
mod m20260827_000004_restore_menu_parent_default;
mod m20260901_000005_create_sys_dict;
mod m20260902_000006_add_user_emp_no;
mod m20260903_000007_create_sys_operation_log;
mod m20260903_000008_create_sys_login_log;
mod m20260903_000009_create_sys_dictionary;
mod m20260904_000010_add_audit_columns;
mod m20260905_000011_create_sys_file;
mod m20260905_000012_create_sys_config_and_sys_site_config;
mod m20260907_000013_create_sys_job_and_sys_job_log;
mod m20260908_000014_add_role_api_api_index;
mod m20260908_000015_add_role_menu_menu_index;
mod m20260908_000016_normalize_index_names;
mod m20260909_000017_add_missing_column_table_comments;
mod m20260909_000018_create_sys_dept;
mod m20260909_000019_create_sys_user_dept;
mod m20260909_000020_drop_sys_dept_leader_phone_email;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260825_000001_create_rbac_tables::Migration),
            Box::new(m20260827_000002_add_rbac_column_comments::Migration),
            Box::new(m20260827_000003_add_rbac_table_comments::Migration),
            Box::new(m20260827_000004_restore_menu_parent_default::Migration),
            Box::new(m20260901_000005_create_sys_dict::Migration),
            Box::new(m20260902_000006_add_user_emp_no::Migration),
            Box::new(m20260903_000007_create_sys_operation_log::Migration),
            Box::new(m20260903_000008_create_sys_login_log::Migration),
            Box::new(m20260903_000009_create_sys_dictionary::Migration),
            Box::new(m20260904_000010_add_audit_columns::Migration),
            Box::new(m20260905_000011_create_sys_file::Migration),
            Box::new(m20260905_000012_create_sys_config_and_sys_site_config::Migration),
            Box::new(m20260907_000013_create_sys_job_and_sys_job_log::Migration),
            Box::new(m20260908_000014_add_role_api_api_index::Migration),
            Box::new(m20260908_000015_add_role_menu_menu_index::Migration),
            Box::new(m20260908_000016_normalize_index_names::Migration),
            Box::new(m20260909_000017_add_missing_column_table_comments::Migration),
            Box::new(m20260909_000018_create_sys_dept::Migration),
            Box::new(m20260909_000019_create_sys_user_dept::Migration),
            Box::new(m20260909_000020_drop_sys_dept_leader_phone_email::Migration),
        ]
    }
}
