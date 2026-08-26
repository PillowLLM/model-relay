use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use relay_core::AppError;
use serde_json::json;

/// 包装 AppError 以绕过孤儿规则（AppError 定义在 relay-core 中）。
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let st = self.0.status();
        let body = Json(json!({ "error": self.0.message(), "code": st }));
        (StatusCode::from_u16(st).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), body).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
