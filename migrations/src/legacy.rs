//! 历史迁移版本占位（2026-08-25 ~ 2026-09-09 共 25 条）。
//!
//! 真实 DDL 已压平进 `m20260913_000001_baseline`（由旧迁移最终态导出），
//! 这些空操作占位仅用于满足 sea-orm 的存在性校验：seaql_migrations 里
//! 记录了这 25 个版本号的库（历史部署环境）在升级后仍能通过
//! `migration up` 的版本比对，否则会报「migration file is missing」拒绝启动。
//! 新库上它们按顺序空跑后由 baseline 一次性建表，效果一致。
//!
//! 注意：本文件不可删除，除非所有环境的 seaql_migrations 已清理旧版本号
//! （那需要一次性人工 SQL，且生产升级有忘记执行的风险）。

pub mod m20260825_000001_create_rbac_tables {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260825_000001_create_rbac_tables"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260827_000002_add_rbac_column_comments {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260827_000002_add_rbac_column_comments"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260827_000003_add_rbac_table_comments {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260827_000003_add_rbac_table_comments"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260827_000004_restore_menu_parent_default {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260827_000004_restore_menu_parent_default"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260901_000005_create_sys_dict {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260901_000005_create_sys_dict"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260902_000006_add_user_emp_no {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260902_000006_add_user_emp_no"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260903_000007_create_sys_operation_log {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260903_000007_create_sys_operation_log"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260903_000008_create_sys_login_log {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260903_000008_create_sys_login_log"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260903_000009_create_sys_dictionary {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260903_000009_create_sys_dictionary"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260904_000010_add_audit_columns {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260904_000010_add_audit_columns"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260905_000011_create_sys_file {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260905_000011_create_sys_file"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260905_000012_create_sys_config_and_sys_site_config {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260905_000012_create_sys_config_and_sys_site_config"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260907_000013_create_sys_job_and_sys_job_log {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260907_000013_create_sys_job_and_sys_job_log"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260908_000014_add_role_api_api_index {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260908_000014_add_role_api_api_index"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260908_000015_add_role_menu_menu_index {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260908_000015_add_role_menu_menu_index"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260908_000016_normalize_index_names {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260908_000016_normalize_index_names"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000017_add_missing_column_table_comments {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000017_add_missing_column_table_comments"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000018_create_sys_dept {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000018_create_sys_dept"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000019_create_sys_user_dept {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000019_create_sys_user_dept"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000020_drop_sys_dept_leader_phone_email {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000020_drop_sys_dept_leader_phone_email"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000021_add_user_dept_primary_unique {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000021_add_user_dept_primary_unique"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000022_add_sys_menu_name_unique {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000022_add_sys_menu_name_unique"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000023_widen_sys_dept_path {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000023_widen_sys_dept_path"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000024_create_sys_position {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000024_create_sys_position"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}

pub mod m20260909_000025_create_sys_user_position {
    use sea_orm_migration::prelude::*;

    pub struct Migration;

    /// DeriveMigrationName 会取 module_path 第二段（这里是 `legacy`），
    /// 嵌套模块下名字不对，因此手写版本号字面量
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260909_000025_create_sys_user_position"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }
    }
}
