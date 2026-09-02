//! 请求体提取器：字段级错误提示版 `JsonBody<T>`。
//!
//! 背景：Salvo 内置 `JsonBody` 反序列化失败时只产出 HTTP 400，全局 catcher 兜底为
//! 固定文案「请求参数格式错误」，前端无法得知具体是哪个字段出错。本模块提供等价
//! 提取器：解析时用 `serde_path_to_error` 记录出错字段路径，把错误翻译成中文提示
//! （如「字段 age 类型错误」「缺少必填字段 username」），再以 `StatusError` 交给
//! 全局 catcher，保持 HTTP 200 + `code: 0` 契约体不变。
//!
//! 限制：`#[serde(flatten)]`（如分页 `PageQuery`）会丢失字段路径，该类错误只能给出
//! 「请求体不符合接口要求」这类通用提示，无法定位到具体字段。
//!
//! 请求出错时的完整链路（学习用）：
//! ```text
//! 客户端 POST JSON
//!   │
//!   ▼
//! #[endpoint] 宏先提取参数（调用本模块的 JsonBody::extract）
//!   │ 反序列化失败
//!   ▼
//! BodyParamError::write 渲染 StatusError(400)，并把中文提示放进 origin
//!   │（Salvo 看到 4xx + ResBody::Error 时不会直接返回，而是交给 catcher）
//!   ▼
//! infra/catcher 的 handle_error 从 origin 取回提示，改写为 HTTP 200 + code 0 契约体
//! ```

use std::fmt;
use std::ops::{Deref, DerefMut};

use salvo::extract::{Extractible, Metadata};
use salvo::http::mime;
use salvo::http::{ParseError, StatusError};
use salvo::oapi::{
    Components, Content, EndpointArgRegister, Operation, RequestBody, ToRequestBody, ToSchema,
};
use salvo::prelude::*;
use serde::Deserialize;
use serde_path_to_error as path_to_error;

const ERROR_PREFIX: &str = "请求参数格式错误：";

/// 与 Salvo 内置 `JsonBody<T>` 用法一致的请求体提取器（含字段定位错误提示）。
pub struct JsonBody<T>(pub T);

impl<T> JsonBody<T> {
    /// 取出内部值，与 Salvo 内置用法保持一致。
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for JsonBody<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for JsonBody<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'ex, T> Extractible<'ex> for JsonBody<T>
where
    T: Deserialize<'ex> + Send,
{
    // Extractible 是 Salvo 的“提取器”接口：handler 参数里写的
    // JsonBody<T> 会被 #[endpoint] 宏在调用业务函数之前先“提取”出来。
    // 我们复刻 Salvo 内置 JsonBody 的这组 trait 实现，只是把解析换成
    // 带字段路径的版本，因此业务代码（handler 签名、into_inner、字段访问）零改动。
    fn metadata() -> &'static Metadata {
        static METADATA: Metadata = Metadata::new("");
        &METADATA
    }

    async fn extract(
        req: &'ex mut Request,
        _depot: &'ex mut Depot,
    ) -> Result<Self, impl Writer + Send + fmt::Debug + 'static> {
        extract_json_body(req).await.map(Self)
    }
}

impl<'de, T> ToRequestBody for JsonBody<T>
where
    T: Deserialize<'de> + ToSchema,
{
    fn to_request_body(components: &mut Components) -> RequestBody {
        RequestBody::new()
            .description("Extract json format data from request.")
            .add_content("application/json", Content::new(T::to_schema(components)))
    }
}

impl<'de, T> EndpointArgRegister for JsonBody<T>
where
    T: Deserialize<'de> + ToSchema,
{
    fn register(components: &mut Components, operation: &mut Operation, _arg: &str) {
        let request_body = Self::to_request_body(components);
        let _ = <T as ToSchema>::to_schema(components);
        operation.request_body = Some(request_body);
    }
}

/// 字段级错误详情。catcher 通过 `StatusError::downcast_origin` 取回并原样透传。
#[derive(Debug, Clone)]
pub struct ParamErrorDetail(String);

impl ParamErrorDetail {
    /// 完整中文提示（可直接作为契约体 `message` 展示）。
    pub fn message(&self) -> &str {
        &self.0
    }
}

/// 提取失败错误：Writer 渲染为带详细 origin 的 400，交给全局 catcher 统一契约体。
#[derive(Debug)]
struct BodyParamError {
    message: String,
}

impl BodyParamError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait]
impl Writer for BodyParamError {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        // 关键机制：不直接写 JSON 响应体，而是渲染成一个“错误响应”（ResBody::Error）。
        // - StatusError::bad_request() 给 HTTP 400：endpoint 宏只会在状态码不是
        //   4xx/5xx 时强制补 400，这里先给 400 可以避免它覆盖我们写的响应；
        // - origin(...) 把完整中文提示塞进 StatusError 的类型化字段：catcher 可以
        //   用 downcast_origin 精确取回，不需要解析字符串；
        // - Salvo 服务层遇到 4xx + ResBody::Error 会调用全局 catcher 收尾，
        //   由 catcher 统一渲染成 HTTP 200 + { code: 0, data, message } 契约体。
        res.render(StatusError::bad_request().origin(ParamErrorDetail(self.message)));
    }
}

async fn extract_json_body<'de, T>(req: &'de mut Request) -> Result<T, BodyParamError>
where
    T: Deserialize<'de>,
{
    let is_json = req
        .content_type()
        .is_some_and(|ct| ct.subtype() == mime::JSON);
    if !is_json {
        return Err(BodyParamError::new(format!(
            "{ERROR_PREFIX}Content-Type 必须为 application/json"
        )));
    }

    let payload = req
        .payload_with_max_size(req.secure_max_size())
        .await
        .map_err(|err| BodyParamError::new(map_payload_error(&err)))?;
    if payload.is_empty() {
        return Err(BodyParamError::new(format!("{ERROR_PREFIX}请求体不能为空")));
    }

    let mut deserializer = serde_json::Deserializer::from_slice(payload);
    // 为什么需要 serde_path_to_error：serde_json 原生错误对“类型不对”只给
    // 「invalid type: ... expected u8 at line 1 column 25」，没有字段名；
    // 包一层路径追踪后，错误会带上 age / nested.code / roles[0] 这样的字段路径。
    match path_to_error::deserialize::<_, T>(&mut deserializer) {
        Ok(value) => Ok(value),
        Err(err) => {
            // 完整原文留日志，便于排查模板没覆盖到的罕见错误；用户看到的只取翻译后的提示。
            tracing::warn!(path = %err.path(), error = %err.inner(), "json body 反序列化失败");
            Err(BodyParamError::new(friendly_serde_message(
                &err.path().to_string(),
                err.inner(),
            )))
        }
    }
}

fn map_payload_error(err: &ParseError) -> String {
    match err {
        ParseError::PayloadTooLarge => format!("{ERROR_PREFIX}请求体超过大小限制"),
        _ => format!("{ERROR_PREFIX}请求体读取失败"),
    }
}

/// 把 serde 解析错误转成带字段的中文提示。
fn friendly_serde_message(path: &str, err: &serde_json::Error) -> String {
    if err.is_syntax() || err.is_eof() {
        return format!(
            "{ERROR_PREFIX}JSON 语法错误（第 {} 行第 {} 列附近）",
            err.line(),
            err.column()
        );
    }
    if err.is_io() {
        return format!("{ERROR_PREFIX}请求体读取失败");
    }
    let raw = err.to_string();
    let reason = raw.split(" at line ").next().unwrap_or(&raw);
    describe_data_error(normalize_path(path), reason)
}

fn normalize_path(path: &str) -> &str {
    // serde_path_to_error 对“根对象”的路径显示为 "."，这里归一化成空串，
    // 后面统一用 path.is_empty() 判断“错误发生在根级还是某个字段里”。
    if path == "." { "" } else { path }
}

/// 把 serde 的英文错误（去掉行列号后）按模式翻译成中文。
///
/// 常见原文示例（学习用）：
/// - `missing field \`username\``          → 缺字段，字段名在反引号里
/// - `invalid type: string "x", expected u8` → 类型错，期望类型是 Rust 类型名
/// - `invalid value: integer \`300\`, expected u8` → 值越界（不是类型错）
/// - `unknown variant \`a\`, expected one of \`x\`, \`y\`` → 枚举值不合法
fn describe_data_error(path: &str, reason: &str) -> String {
    if let Some(field) = backticked_after(reason, "missing field ") {
        return if path.is_empty() {
            format!("{ERROR_PREFIX}缺少必填字段 {field}")
        } else {
            format!("{ERROR_PREFIX}字段 {path} 中缺少必填字段 {field}")
        };
    }
    if let Some(field) = backticked_after(reason, "duplicate field ") {
        return if path.is_empty() {
            format!("{ERROR_PREFIX}字段 {field} 重复")
        } else {
            format!("{ERROR_PREFIX}字段 {path}.{field} 重复")
        };
    }
    if let Some(field) = backticked_after(reason, "unknown field ") {
        return format!("{ERROR_PREFIX}存在未知字段 {field}");
    }

    if path.is_empty() {
        if reason.contains("expected struct") || reason.contains("expected a map") {
            return format!("{ERROR_PREFIX}请求体应为 JSON 对象");
        }
        if reason.contains("expected a sequence") || reason.contains("expected seq") {
            return format!("{ERROR_PREFIX}请求体应为 JSON 数组");
        }
        // 走到这里通常是根级结构问题，或 #[serde(flatten)] 把子结构拍平后
        // 丢失了字段路径（如 PageQuery 的 page 字段）。没有路径就只给通用提示。
        return format!("{ERROR_PREFIX}请求体不符合接口要求");
    }

    let field_prefix = format!("{ERROR_PREFIX}字段 {path}");
    if reason.contains("unknown variant") {
        return match variant_options(reason) {
            Some(options) => format!("{field_prefix} 取值不合法，可选值：{options}"),
            None => format!("{field_prefix} 取值不合法"),
        };
    }
    if reason.starts_with("invalid type:") {
        let actual = attach_phrase("实际为", actual_kind_zh(invalid_type_actual(reason)));
        return match expected_phrase(reason) {
            Some(expected) => format!("{field_prefix} 类型错误，{expected}，{actual}"),
            None => format!("{field_prefix} 类型错误，{actual}"),
        };
    }
    if reason.starts_with("invalid value:") {
        return match expected_phrase(reason) {
            Some(expected) => format!("{field_prefix} 取值不合法，{expected}"),
            None => format!("{field_prefix} 取值不合法，超出允许范围"),
        };
    }
    format!("{field_prefix} 格式不正确")
}

/// 取 `expected ...` 段并翻译成「应为……」的中文短语。
fn expected_phrase(reason: &str) -> Option<String> {
    let raw = reason
        .rsplit_once("expected ")
        .map(|(_, expected)| expected.trim())
        .filter(|expected| !expected.is_empty())?;
    expected_type_zh(raw).map(|zh| attach_phrase("应为", &zh))
}

/// 动词 + 名词拼接：名词以 ASCII 开头时补一个空格（如「应为 0 到 255 之间的整数」），
/// 纯中文时直接相连（如「应为整数」）。
fn attach_phrase(verb: &str, noun: &str) -> String {
    let starts_with_ascii = noun
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric());
    if starts_with_ascii {
        format!("{verb} {noun}")
    } else {
        format!("{verb}{noun}")
    }
}

fn expected_type_zh(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix("a ")
        .or_else(|| trimmed.strip_prefix("an "))
        .unwrap_or(trimmed);
    let zh = match trimmed {
        "u8" => "0 到 255 之间的整数",
        "i8" => "-128 到 127 之间的整数",
        "u16" | "u32" | "u64" | "u128" | "usize" | "i16" | "i32" | "i64" | "i128" | "isize"
        | "integer" => "整数",
        "f32" | "f64" => "数字（可含小数）",
        "string" | "&str" => "字符串",
        "boolean" | "bool" => "布尔值（true 或 false）",
        "sequence" | "seq" => "数组",
        "map" => "JSON 对象",
        "char" => "单个字符",
        "unit" => "空值",
        _ if trimmed.starts_with("struct ") || trimmed.starts_with("enum ") => "JSON 对象",
        _ => return None,
    };
    Some(zh.to_string())
}

fn invalid_type_actual(reason: &str) -> Option<&str> {
    let rest = reason.strip_prefix("invalid type:")?;
    rest.split_once(',')
        .map(|(actual, _)| actual.trim())
        .filter(|actual| !actual.is_empty())
}

fn actual_kind_zh(actual: Option<&str>) -> &'static str {
    let token = actual
        .and_then(|raw| raw.split_whitespace().next())
        .unwrap_or("");
    // 只回传“值是什么类型”，不回传值本身：避免把密码/手机号等敏感内容回显进错误消息。
    match token {
        "string" => "字符串",
        "integer" => "整数",
        "boolean" => "布尔值",
        "null" => "null",
        "sequence" => "数组",
        "map" => "对象",
        "unit" => "空值",
        "char" => "字符",
        _ => "其他类型",
    }
}

fn backticked_after<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(prefix)?;
    let start = rest.find('`')? + 1;
    let tail = &rest[start..];
    let end = tail.find('`')? + start;
    Some(&rest[start..end])
}

fn variant_options(reason: &str) -> Option<String> {
    let rest = reason.split("expected one of ").nth(1)?;
    let options: Vec<&str> = rest
        .split('`')
        .filter(|part| !part.is_empty() && !part.starts_with(','))
        .collect();
    if options.is_empty() {
        None
    } else {
        Some(options.join("、"))
    }
}

#[cfg(test)]
mod tests {
    use salvo::oapi::endpoint;
    use salvo::prelude::*;
    use salvo::test::{ResponseExt, TestClient};
    use serde::Deserialize;
    use serde_json::Value;

    use super::JsonBody;
    use crate::infra::catcher;

    #[derive(Debug, Deserialize, ToSchema)]
    #[allow(dead_code)] // 仅用于验证反序列化契约，字段不参与断言
    struct UpdateReq {
        pub id: u64,
        pub username: String,
        pub age: u8,
        pub roles: Vec<String>,
        pub nested: NestedReq,
    }

    #[derive(Debug, Deserialize, ToSchema)]
    #[allow(dead_code)] // 仅用于验证反序列化契约，字段不参与断言
    struct NestedReq {
        pub code: i32,
    }

    #[endpoint]
    async fn simulate_update(depot: &mut Depot, body: JsonBody<UpdateReq>) -> String {
        let _ = (depot, body);
        "ok".to_string()
    }

    fn router() -> Router {
        Router::with_path("api/v1/user/update").post(simulate_update)
    }

    fn service() -> Service {
        Service::new(router()).catcher(catcher::build())
    }

    fn valid_body() -> &'static str {
        r#"{"id":1,"username":"a","age":1,"roles":["x"],"nested":{"code":1}}"#
    }

    async fn post(body: &str, content_type: &str) -> (StatusCode, String) {
        let mut res = TestClient::post("http://test/api/v1/user/update")
            .body(body.to_string())
            .add_header("content-type", content_type, true)
            .send(&service())
            .await;
        let status = res.status_code.unwrap();
        let text = res.take_string().await.unwrap();
        (status, text)
    }

    async fn error_message(body: &str) -> String {
        let (status, text) = post(body, "application/json").await;
        let parsed: Value = serde_json::from_str(&text).expect("响应体应为合法 JSON");
        assert_eq!(status, StatusCode::OK, "框架级错误应统一为 HTTP 200");
        assert_eq!(parsed["code"], 0, "错误应带 code=0");
        parsed["message"]
            .as_str()
            .expect("契约体应包含 message 字符串")
            .to_string()
    }

    #[tokio::test]
    async fn valid_body_reaches_handler() {
        let (status, text) = post(valid_body(), "application/json").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(text, "ok");
    }

    #[tokio::test]
    async fn missing_field_reports_field_name() {
        let body = r#"{"id":1,"age":1,"roles":[],"nested":{"code":1}}"#;
        let message = error_message(body).await;
        assert_eq!(message, "请求参数格式错误：缺少必填字段 username");
    }

    #[tokio::test]
    async fn wrong_type_reports_field_and_kinds() {
        let body = r#"{"id":1,"username":"a","age":"x","roles":[],"nested":{"code":1}}"#;
        let message = error_message(body).await;
        assert_eq!(
            message,
            "请求参数格式错误：字段 age 类型错误，应为 0 到 255 之间的整数，实际为字符串"
        );
    }

    #[tokio::test]
    async fn nested_type_error_reports_nested_path() {
        let body = r#"{"id":1,"username":"a","age":1,"roles":[],"nested":{"code":"x"}}"#;
        let message = error_message(body).await;
        assert_eq!(
            message,
            "请求参数格式错误：字段 nested.code 类型错误，应为整数，实际为字符串"
        );
    }

    #[tokio::test]
    async fn vec_element_type_error_reports_index() {
        let body = r#"{"id":1,"username":"a","age":1,"roles":[1],"nested":{"code":1}}"#;
        let message = error_message(body).await;
        assert_eq!(
            message,
            "请求参数格式错误：字段 roles[0] 类型错误，应为字符串，实际为整数"
        );
    }

    #[tokio::test]
    async fn out_of_range_value_reports_rule() {
        let body = r#"{"id":1,"username":"a","age":300,"roles":[],"nested":{"code":1}}"#;
        let message = error_message(body).await;
        assert_eq!(
            message,
            "请求参数格式错误：字段 age 取值不合法，应为 0 到 255 之间的整数"
        );
    }

    #[tokio::test]
    async fn malformed_json_reports_syntax_error() {
        let message = error_message(r#"{"id":1,"username":"a""#).await;
        assert!(
            message.contains("JSON 语法错误"),
            "语法错误应提示 JSON 语法错误，实际为：{message}"
        );
    }

    #[tokio::test]
    async fn root_not_object_reports_object_expected() {
        let message = error_message(r#""hello""#).await;
        assert_eq!(message, "请求参数格式错误：请求体应为 JSON 对象");
    }

    #[tokio::test]
    async fn empty_body_reports_empty() {
        let message = error_message("").await;
        assert_eq!(message, "请求参数格式错误：请求体不能为空");
    }

    #[tokio::test]
    async fn non_json_content_type_reports_content_type() {
        let (status, text) = post("{}", "text/plain").await;
        let parsed: Value = serde_json::from_str(&text).expect("响应体应为合法 JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["code"], 0);
        assert_eq!(
            parsed["message"],
            "请求参数格式错误：Content-Type 必须为 application/json"
        );
    }
}
