//! 文件 handler。
//!
//! 端点顺序 = `mod.rs` 路由顺序：`list → upload → get → download → delete`。
//! 上传为 multipart（POST）、下载为 GET + query，是「POST + JSON body」契约的两个例外。

use std::path::Path;

use salvo::fs::NamedFile;
use salvo::oapi::endpoint;
use salvo::prelude::*;

use crate::infra::state::AppState;
use crate::middleware::auth::AuthUser;
use crate::modules::file::dto::{FileListReq, FileResp, FileUploadResp, UploadInput};
use crate::modules::file::service as file_service;
use crate::utils::error::AppError;
use crate::utils::request::JsonBody;
use crate::utils::user_ref::fill_user_names;
use crate::utils::{ApiResponse, ApiResult, IdReq, PageResult};

/// 文件列表（POST + JSON body）。
#[endpoint]
pub async fn list_files(
    depot: &mut Depot,
    body: JsonBody<FileListReq>,
) -> ApiResult<PageResult<FileResp>> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let data = file_service::page_files(&state.db, &req).await?;
    let items = fill_user_names(&state.db, data.items, FileResp::from).await?;
    Ok(ApiResponse::ok(PageResult::new(
        data.total,
        data.total_pages,
        items,
    )))
}

/// 上传文件（multipart，字段名 `file`）。
///
/// 「POST + JSON body」契约的例外之一：multipart 由浏览器 Upload 组件原生产生，
/// 强行 base64 塞进 JSON 只会浪费内存。鉴权仍走本路由组的 AuthRequired。
#[endpoint]
pub async fn upload_file(depot: &mut Depot, req: &mut Request) -> ApiResult<FileUploadResp> {
    let state = AppState::from_depot(depot)?;
    // 上传人来自 JWT（AuthRequired 注入），请求体永远不传人字段（防伪造）
    let auth = AuthUser::from_depot(depot)?;

    // 解析 multipart：salvo 把每个文件部分写到临时目录，解析错误多为
    // Content-Type 非 multipart 或请求体超限，都属客户端错误 → Biz
    let form = req
        .form_data()
        .await
        .map_err(|e| AppError::Biz(format!("上传请求解析失败：{e}")))?;
    // 只认字段名 `file`；MultiMap::get 取第一个同名部分
    let Some(file) = form.files.get("file") else {
        return Err(AppError::Biz("未选择文件".to_string()));
    };

    // 从 FilePart 摘出 service 需要的四项（全是拷贝，借用即止）：
    // - name：浏览器带来的原始文件名，ext 由 service 从这里推导并校验白名单
    // - mime：仅作记录展示，不作为安全依据（客户端可伪造）
    // - temp_path：临时文件路径，请求结束后 salvo 自动清理，service 必须 rename/copy 转存
    let input = UploadInput {
        name: file.name().unwrap_or_default().to_string(),
        mime: file
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_default(),
        size: file.size(),
        temp_path: file.path().clone(),
    };

    // 上传限制全部来自 [upload] 配置，handler 只负责把配置喂给 service
    let upload = &state.config.upload;
    let model = file_service::upload_file(
        &state.db,
        Path::new(&upload.dir),
        upload.max_size_bytes(),
        &upload.allows,
        auth.user_id,
        input,
    )
    .await?;

    // url 用相对路径：前端拼站点域名后可直接用，反向代理改前缀也不用改后端；
    // created_by_name 与列表契约一致（规格 §5：上传响应在列表字段基础上含 url）
    let url = format!("/api/v1/file/download?id={}", model.id);
    let mut resp = fill_user_names(&state.db, vec![model], FileResp::from).await?;
    let file = resp.remove(0);
    Ok(ApiResponse::ok(FileUploadResp { file, url }))
}

/// 文件详情（POST + JSON body：`{ "id": ... }`）。
#[endpoint]
pub async fn get_file(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<FileResp> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    let file = file_service::get_file(&state.db, req.id).await?;
    let mut resp = fill_user_names(&state.db, vec![file], FileResp::from).await?;
    Ok(ApiResponse::ok(resp.remove(0)))
}

/// 鉴权下载（GET + query `id`）：`NamedFile` 流式返回，非契约 JSON 体，故用 #[handler]
/// 而非 #[endpoint]（NamedFile 未实现 OpenAPI 注册）。
///
/// 例外之二的 GET：浏览器直接以 `<img src>` / `window.open` 发起下载时无法
/// 自定义 JSON body，只能走 query。响应头全部从记录取，禁止信任前端传路径。
#[handler]
pub async fn download_file(depot: &mut Depot, req: &mut Request) -> Result<NamedFile, AppError> {
    let state = AppState::from_depot(depot)?;
    let id: u64 = req.query("id").unwrap_or(0);
    // id → 记录 → stored_name → 磁盘路径，全链路服务端拼装；
    // 前端传任何路径参数都到不了这里（接口只收 id）
    let (model, path) =
        file_service::download_path(&state.db, Path::new(&state.config.upload.dir), id).await?;

    // attached_name 生成 `Content-Disposition: attachment; filename=...`，
    // 含非 ASCII（中文文件名）时自动追加 filename*=UTF-8'' 百分号编码，无需手工处理
    NamedFile::builder(path)
        .attached_name(&model.name)
        .content_type(
            model
                .mime
                .parse()
                .unwrap_or(salvo::http::mime::APPLICATION_OCTET_STREAM),
        )
        .build()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("读取下载文件失败：{e}")))
}

/// 删除文件（POST + JSON body：`{ "id": ... }`）：软删记录 + 物理删磁盘文件。
#[endpoint]
pub async fn delete_file(depot: &mut Depot, body: JsonBody<IdReq>) -> ApiResult<()> {
    let state = AppState::from_depot(depot)?;
    let req = body.into_inner();
    file_service::delete_file(&state.db, Path::new(&state.config.upload.dir), req.id).await?;
    Ok(ApiResponse::ok(()))
}
