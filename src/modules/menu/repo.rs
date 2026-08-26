//! 菜单域数据访问。

use crate::entity::{sys_menu, sys_menu::Model};
use sea_orm::entity::prelude::*;
use sea_orm::{DatabaseConnection, QueryOrder};

/// 查询全部启用菜单（W2 超管全量菜单树；按角色过滤 W3 再做）。
pub async fn find_all_menus(db: &DatabaseConnection) -> anyhow::Result<Vec<Model>> {
    Ok(sys_menu::Entity::find()
        .filter(sys_menu::Column::Status.eq(1))
        .filter(sys_menu::Column::DeletedAt.is_null())
        .order_by_asc(sys_menu::Column::Sort)
        .order_by_asc(sys_menu::Column::Id)
        .all(db)
        .await?)
}
