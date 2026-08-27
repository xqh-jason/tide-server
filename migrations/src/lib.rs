pub use sea_orm_migration::prelude::*;

mod m20260825_000001_create_rbac_tables;
mod m20260827_000002_add_rbac_column_comments;
mod m20260827_000003_add_rbac_table_comments;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260825_000001_create_rbac_tables::Migration),
            Box::new(m20260827_000002_add_rbac_column_comments::Migration),
            Box::new(m20260827_000003_add_rbac_table_comments::Migration),
        ]
    }
}
