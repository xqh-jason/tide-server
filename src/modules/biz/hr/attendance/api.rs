//! 考勤 handler（端点函数）。
//!
//! 函数顺序 = `mod.rs` 路由挂载顺序：`shift` list → create → update → get → delete，
//! `schedule` list → batch-create → update → month，`record` list → get → update → import，
//! `calendar` list → upsert → batch-import。
//!
//! handler 只做三件事：取状态（`AppState`）/ 取操作人（`AuthUser`）→ 预取值域（字典）
//! → 调 validate + service → 拼显示名（`fill_user_names` 走平台唯一管道；`employee_name` 走
//! 员工域 `employee_service::find_employee_name_map`；`shift_code` / `shift_name` 走本域
//! `shift_briefs`）。
//! 业务规则不写在这里。

use salvo::oapi::endpoint;
use salvo::prelude::*;
use sea_orm::ConnectionTrait;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::biz::hr::attendance::dto::{
    BatchCreateScheduleReq, BatchCreateScheduleResp, BatchImportCalendarReq,
    BatchImportCalendarResp, CalendarListReq, CalendarResp, CreateShiftReq, ImportRecordReq,
    ImportRecordResp, RecordListReq, RecordResp, ScheduleListReq, ScheduleMonthReq,
    ScheduleMonthResp, ScheduleResp, ShiftListReq, ShiftResp, UpdateRecordReq, UpdateScheduleReq,
    UpdateShiftReq, UpsertCalendarReq,
};
use crate::modules::biz::hr::attendance::{service as attendance_service, validate};
use crate::modules::biz::hr::employee::service as employee_service;
use crate::modules::system::dictionary::service as dictionary_service;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 给排班响应批量回填 `employee_name` / `shift_code` / `shift_name`（各一次批量查）。
async fn fill_schedule_ref_names(
    db: &impl ConnectionTrait,
    items: &mut [ScheduleResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let shift_ids: Vec<u64> = items.iter().map(|item| item.shift_id).collect();
    let employee_names = employee_service::find_employee_name_map(db, &employee_ids).await?;
    let briefs = attendance_service::shift_briefs(db, &shift_ids).await?;

    for item in items.iter_mut() {
        item.employee_name = employee_names
            .get(&item.employee_id)
            .cloned()
            .unwrap_or_default();
        if let Some((code, name)) = briefs.get(&item.shift_id) {
            item.shift_code = code.clone();
            item.shift_name = name.clone();
        }
    }
    Ok(())
}

/// 给出勤事实响应批量回填 `employee_name` / `shift_code` / `shift_name`（各一次批量查）。
async fn fill_record_ref_names(
    db: &impl ConnectionTrait,
    items: &mut [RecordResp],
) -> Result<(), AppError> {
    let employee_ids: Vec<u64> = items.iter().map(|item| item.employee_id).collect();
    let shift_ids: Vec<u64> = items.iter().map(|item| item.shift_id).collect();
    let employee_names = employee_service::find_employee_name_map(db, &employee_ids).await?;
    let briefs = attendance_service::shift_briefs(db, &shift_ids).await?;

    for item in items.iter_mut() {
        item.employee_name = employee_names
            .get(&item.employee_id)
            .cloned()
            .unwrap_or_default();
        if let Some((code, name)) = briefs.get(&item.shift_id) {
            item.shift_code = code.clone();
            item.shift_name = name.clone();
        }
    }
    Ok(())
}

/// 班次列表（POST + JSON body）。
#[endpoint]
pub async fn list_shifts(
    depot: &mut Depot,
    body: JsonBody<ShiftListReq>,
) -> ApiResult<PageResult<ShiftResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = attendance_service::page_shifts(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, ShiftResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 创建班次。
#[endpoint]
pub async fn create_shift(
    depot: &mut Depot,
    body: JsonBody<CreateShiftReq>,
) -> ApiResult<ShiftResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    // status 允许值的唯一来源是平台字典，不硬编码
    let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;
    validate::validate_create_shift(&req, &status_allowed).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = attendance_service::create_shift(&state.db, auth.user_id, req).await?;
    let resp = fill_user_names(&state.db, vec![model], ShiftResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 更新班次。
#[endpoint]
pub async fn update_shift(
    depot: &mut Depot,
    body: JsonBody<UpdateShiftReq>,
) -> ApiResult<ShiftResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let status_allowed = dictionary_service::enabled_int_values(&state.db, "status").await?;
    validate::validate_update_shift(&req, &status_allowed).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = attendance_service::update_shift(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], ShiftResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 班次详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_shift(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<ShiftResp> {
    let state = AppState::from_depot(depot)?;
    let model = attendance_service::get_shift(&state.db, body.into_inner().id).await?;
    let resp = fill_user_names(&state.db, vec![model], ShiftResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 删除班次（软删）。
#[endpoint]
pub async fn delete_shift(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let auth = AuthUser::from_depot(depot)?;
    attendance_service::delete_shift(&state.db, auth.user_id, body.into_inner().id).await?;
    Ok(ApiResponse::ok(()))
}

/// 排班列表（POST + JSON body）。
#[endpoint]
pub async fn list_schedules(
    depot: &mut Depot,
    body: JsonBody<ScheduleListReq>,
) -> ApiResult<PageResult<ScheduleResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = attendance_service::page_schedules(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, ScheduleResp::from).await?;
    fill_schedule_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 批量排班（员工 × 日期区间逐日 upsert）。
#[endpoint]
pub async fn batch_create_schedules(
    depot: &mut Depot,
    body: JsonBody<BatchCreateScheduleReq>,
) -> ApiResult<BatchCreateScheduleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_batch_create_schedules(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let resp = attendance_service::batch_create_schedules(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(resp))
}

/// 单日排班 upsert（`{ employeeId, workDate, shiftId, status, remark }`）。
#[endpoint]
pub async fn update_schedule(
    depot: &mut Depot,
    body: JsonBody<UpdateScheduleReq>,
) -> ApiResult<ScheduleResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_update_schedule(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = attendance_service::update_schedule(&state.db, auth.user_id, &req).await?;
    let mut items = fill_user_names(&state.db, vec![model], ScheduleResp::from).await?;
    fill_schedule_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(items.remove(0)))
}

/// 排班月视图（某月 × 某部门 / 全员；`month` 形如 `2026-09`）。
#[endpoint]
pub async fn month_schedules(
    depot: &mut Depot,
    body: JsonBody<ScheduleMonthReq>,
) -> ApiResult<ScheduleMonthResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    // 员工名与班次名已在 service 内批量拼装（网格行数多，不再走 `fill_user_names`）
    let resp = attendance_service::month_schedules(&state.db, &req).await?;
    Ok(ApiResponse::ok(resp))
}

/// 出勤事实列表（POST + JSON body）。
#[endpoint]
pub async fn list_records(
    depot: &mut Depot,
    body: JsonBody<RecordListReq>,
) -> ApiResult<PageResult<RecordResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = attendance_service::page_records(&state.db, &req).await?;
    let mut items = fill_user_names(&state.db, data.items, RecordResp::from).await?;
    fill_record_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 出勤事实详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_record(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<RecordResp> {
    let state = AppState::from_depot(depot)?;
    let model = attendance_service::get_record(&state.db, body.into_inner().id).await?;
    let mut items = fill_user_names(&state.db, vec![model], RecordResp::from).await?;
    fill_record_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(items.remove(0)))
}

/// 手工补录 / 修正出勤事实（按当前排班口径重算派生列）。
#[endpoint]
pub async fn update_record(
    depot: &mut Depot,
    body: JsonBody<UpdateRecordReq>,
) -> ApiResult<RecordResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_update_record(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = attendance_service::update_record(&state.db, auth.user_id, &req).await?;
    let mut items = fill_user_names(&state.db, vec![model], RecordResp::from).await?;
    fill_record_ref_names(&state.db, &mut items).await?;
    Ok(ApiResponse::ok(items.remove(0)))
}

/// 出勤事实导入（归一化行数组；单行失败不打断整批，错误随回执返回）。
#[endpoint]
pub async fn import_records(
    depot: &mut Depot,
    body: JsonBody<ImportRecordReq>,
) -> ApiResult<ImportRecordResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_import_records(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let resp = attendance_service::import_records(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(resp))
}

/// 工作日历列表（POST + JSON body）。
#[endpoint]
pub async fn list_calendars(
    depot: &mut Depot,
    body: JsonBody<CalendarListReq>,
) -> ApiResult<PageResult<CalendarResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    let data = attendance_service::page_calendars(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, CalendarResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 单日工作日历 upsert。
#[endpoint]
pub async fn upsert_calendar(
    depot: &mut Depot,
    body: JsonBody<UpsertCalendarReq>,
) -> ApiResult<CalendarResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_upsert_calendar(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let model = attendance_service::upsert_calendar(&state.db, auth.user_id, &req).await?;
    let resp = fill_user_names(&state.db, vec![model], CalendarResp::from)
        .await?
        .remove(0);
    Ok(ApiResponse::ok(resp))
}

/// 区间工作日历导入（逐日 upsert）。
#[endpoint]
pub async fn batch_import_calendars(
    depot: &mut Depot,
    body: JsonBody<BatchImportCalendarReq>,
) -> ApiResult<BatchImportCalendarResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();

    validate::validate_batch_import_calendars(&req).map_err(AppError::Biz)?;

    let auth = AuthUser::from_depot(depot)?;
    let resp = attendance_service::batch_import_calendars(&state.db, auth.user_id, &req).await?;
    Ok(ApiResponse::ok(resp))
}
