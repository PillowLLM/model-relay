use std::fmt;

/// 统一业务错误。业务层只构造 AppError；IntoResponse 在 relay-api 层实现。
#[derive(Debug, Clone)]
pub enum AppError {
    BadRequest(String),          // 400
    Unauthorized(String),        // 401
    Forbidden(String),           // 403
    NotFound(String),            // 404
    RateLimited(String),         // 429（限流/额度不足）
    ProviderError { status: u16, message: String }, // 上游错误（透传状态码）
    Internal(String),            // 500
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn rate_limited(msg: impl Into<String>) -> Self {
        Self::RateLimited(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
    pub fn provider(status: u16, message: impl Into<String>) -> Self {
        Self::ProviderError {
            status,
            message: message.into(),
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::RateLimited(_) => 429,
            Self::ProviderError { status, .. } => *status,
            Self::Internal(_) => 500,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m) | Self::Unauthorized(m) | Self::Forbidden(m) | Self::NotFound(m) | Self::RateLimited(m) | Self::Internal(m) => m,
            Self::ProviderError { message, .. } => message,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for AppError {}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::BadRequest(format!("JSON 解析失败: {e}"))
    }
}
