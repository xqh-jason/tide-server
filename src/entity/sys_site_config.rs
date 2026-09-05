//! `SeaORM` Entity（手写，codegen 不适用单行表）。
//!
//! `sys_site_config` 网站设置：恒单行 `id=1`（由迁移种子保障），
//! 无 create/delete 语义，更新走 `update_site_config` 恒按 id=1 盖章。

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "sys_site_config")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: u64,
    /// 站点名称
    pub name: String,
    /// logo 图片 URL
    pub logo: String,
    /// 浏览器 tab 图标 URL
    pub ico: String,
    /// 水印文字
    pub watermark_text: String,
    /// 水印开关 0/1
    pub watermark_enable: i8,
    /// 水印类型：text / pic
    pub watermark_type: String,
    /// 水印图片 URL
    pub watermark_pic: String,
    /// 主题白黑：white / black
    pub mode: String,
    /// 侧边栏模式：dark / light / head
    pub side_mode: String,
    /// 主题色
    pub color: String,
    /// 创建人 ID
    pub created_by: u64,
    /// 更新人 ID
    pub updated_by: u64,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub deleted_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
