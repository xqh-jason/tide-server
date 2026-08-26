use salvo::oapi::{self, EndpointOutRegister, ToSchema};
use salvo::prelude::*;
use serde::Serialize;

/// 统一响应体：`{ code, data, message }`，`code = 200` 表示成功。
/// ⚠️ vben v5 官方模板默认成功码是 `code === 0`（defaultResponseInterceptor 默认
/// successCode=0），前端 request.ts 必须显式改为 `code === 200`（successCode: 200）
/// 才能对接本契约，不是默认匹配。
/// ToSchema 用于 #[endpoint] 生成 OpenAPI 文档。
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub data: T,
    pub message: String,
}

/// 让 ApiResponse 直接作为 handler 返回类型（Salvo 的 Writer trait）：
/// handler 里写 `ApiResponse::ok(data)` 即可，无需再包 `Json(...)`。
#[async_trait]
impl<T: Serialize + Send + 'static> Writer for ApiResponse<T> {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        res.render(Json(self));
    }
}

/// 让 ApiResponse 作为 `#[endpoint]` 返回类型时，也能生成 200 成功响应的 OpenAPI 文档
/// （原来依赖 `Json<C>` 的 blanket 实现，去掉 Json 包装后需自己实现）。
/// 注：`ToSchema` derive 对泛型参数同时要求 `ToSchema` 与 `ComposeSchema`。
impl<T: ToSchema + oapi::ComposeSchema + 'static> EndpointOutRegister for ApiResponse<T> {
    fn register(components: &mut oapi::Components, operation: &mut oapi::Operation) {
        operation.responses.insert(
            "200",
            oapi::Response::new("success")
                .add_content("application/json", Self::to_schema(components)),
        );
    }
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 200,
            data,
            message: "ok".to_string(),
        }
    }

    pub fn fail(code: i32, message: impl Into<String>) -> Self
    where
        T: Default,
    {
        Self {
            code,
            data: T::default(),
            message: message.into(),
        }
    }
}
