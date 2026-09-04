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
        ]
    }
}
