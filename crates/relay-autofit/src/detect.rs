//! 入站格式识别。
use http::HeaderMap;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundFormat {
    OpenAi,
    Anthropic,
    Gemini,
    Unknown,
}

/// 按端点 + 字段嗅探识别入站格式。
pub fn detect_inbound(path: &str, headers: &HeaderMap, body: &Value) -> InboundFormat {
    match path {
        "/v1/chat/completions" => return InboundFormat::OpenAi,
        "/v1/messages" => return InboundFormat::Anthropic,
        p if p.starts_with("/v1beta/models/") && p.ends_with(":generateContent") => return InboundFormat::Gemini,
        _ => {}
    }
    if body.get("messages").is_some()
        && (body.get("max_tokens").is_some() || headers.contains_key("anthropic-version"))
    {
        InboundFormat::Anthropic
    } else if body.get("messages").is_some() {
        InboundFormat::OpenAi
    } else if body.get("contents").is_some() {
        InboundFormat::Gemini
    } else {
        InboundFormat::Unknown
    }
}
