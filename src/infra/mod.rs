//! 基础设施层：应用启动管线（app）、配置（config）、全局状态（state）、路由组装（router）。
//! 与业务域（modules）分离，业务代码只依赖这里的 config/state。

pub mod app;
pub mod config;
pub mod router;
pub mod state;
