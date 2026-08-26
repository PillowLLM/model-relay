//! relay-autofit: 入站识别 + 格式转换矩阵。
pub mod convert;
pub mod detect;
pub mod parse;
pub mod stream;

pub use convert::{to_client, to_provider};
pub use detect::{detect_inbound, InboundFormat};
pub use parse::{parse_anthropic, parse_openai};
pub use stream::event_to_sse;

use relay_core::{AppError, CanonicalRequest};
use serde_json::Value;

/// 按入站格式解析原始 body。
pub fn parse(fmt: InboundFormat, body: &Value) -> Result<CanonicalRequest, AppError> {
    match fmt {
        InboundFormat::OpenAi => parse_openai(body),
        InboundFormat::Anthropic => parse_anthropic(body),
        InboundFormat::Gemini => Err(AppError::bad_request("Gemini 入站暂未实现（M5）")),
        InboundFormat::Unknown => Err(AppError::bad_request("无法识别的请求格式")),
    }
}
