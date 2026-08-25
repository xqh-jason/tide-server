use crate::entity::{
    prelude::SysUser,
    sys_user::{self, Model},
};
use sea_orm::entity::prelude::*;

pub async fn find_by_username(
    db: &sea_orm::DatabaseConnection,
    username: &str,
) -> anyhow::Result<Option<Model>> {
    let user = SysUser::find()
        .filter(sys_user::Column::Username.eq(username))
        .one(db)
        .await?;

    Ok(user)
}
