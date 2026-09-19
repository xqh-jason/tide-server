//! HR 人事模块：人事业务域容器（员工档案 / 考勤 / 请假 / 招聘 / 绩效 / 薪酬…）。
//!
//! 每个子域一个目录 `biz/hr/<域>/{api, service, repo, dto}` 四件套；
//! 在顶层 `modules/mod.rs` 的 `DOMAINS` 追加一行即可挂载。

pub mod employee;
