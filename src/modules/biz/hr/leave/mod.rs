//! HR 假期额度域：假期类型 + 额度账本（授予批次 / 聚合账户 / append-only 流水）。

pub mod dto;
pub mod repo;
pub mod service;

// —— 假期额度域常量唯一定义点（复制到别处即漂移）——
pub const LEAVE_TYPE_ENABLED: i8 = 1;
pub const GRANT_STATUS_ACTIVE: i8 = 1;
pub const GRANT_STATUS_EXHAUSTED: i8 = 2;
pub const GRANT_STATUS_EXPIRED: i8 = 3;
pub const GRANT_STATUS_CANCELED: i8 = 4;
pub const GRANT_SOURCE_ISSUE: i8 = 1;
pub const GRANT_SOURCE_MANUAL: i8 = 2;
pub const GRANT_SOURCE_OVERTIME: i8 = 3;
pub const BALANCE_MODE_DEDUCT: i8 = 1;
pub const BALANCE_MODE_RECORD_ONLY: i8 = 0;
pub const LOG_BIZ_GRANT: i8 = 1;
pub const LOG_BIZ_ADJUST: i8 = 2;
pub const LOG_BIZ_LEAVE_LOCK: i8 = 3;
pub const LOG_BIZ_LEAVE_CONSUME: i8 = 4;
pub const LOG_BIZ_LEAVE_RELEASE: i8 = 5;
pub const LOG_BIZ_EXPIRE: i8 = 6;
