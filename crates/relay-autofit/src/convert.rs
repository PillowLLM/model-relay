//! 出站转换：canonical → 上游原生 / 客户端格式。
use relay_core::*;
use serde_json::{json, Map, Value};

use crate::detect::InboundFormat;

/// 按渠道类型转换出站请求体。
pub fn to_provider(req: &CanonicalRequest, provider_type: &str) -> Result<Value, AppError> {
    match provider_type {
        "anthropic" => Ok(to_anthropic(req)),
        "gemini" => Err(AppError::bad_request("Gemini 出站暂未实现（M5）")),
        _ => Ok(to_openai(req)),
    }
}

/// canonical → OpenAI 兼容请求体。
pub fn to_openai(req: &CanonicalRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(sys) = &req.system {
        if !sys.is_empty() {
            messages.push(json!({ "role": "system", "content": sys }));
        }
    }
    for m in &req.messages {
        // 工具结果 → tool 消息
        if m.content.len() == 1 && matches!(m.content[0], ContentPart::ToolResult { .. }) {
            if let ContentPart::ToolResult { id, content, .. } = &m.content[0] {
                messages.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
                continue;
            }
        }
        let (text_parts, tool_uses): (Vec<&ContentPart>, Vec<&ContentPart>) =
            m.content.iter().partition(|p| matches!(p, ContentPart::Text { .. } | ContentPart::Image { .. }));

        let content = if text_parts.len() == 1 {
            if let ContentPart::Text { text } = text_parts[0] {
                Value::String(text.clone())
            } else {
                Value::Array(parts_to_openai(&text_parts))
            }
        } else if text_parts.is_empty() {
            Value::Null
        } else {
            Value::Array(parts_to_openai(&text_parts))
        };

        let mut msg = json!({ "role": m.role, "content": content });
        if !tool_uses.is_empty() {
            let calls: Vec<Value> = tool_uses
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if let ContentPart::ToolUse { id, name, input } = p {
                        json!({
                            "id": id, "type": "function", "index": i,
                            "function": { "name": name, "arguments": input.to_string() }
                        })
                    } else {
                        json!({})
                    }
                })
                .collect();
            msg["tool_calls"] = Value::Array(calls);
        }
        messages.push(msg);
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("messages".into(), Value::Array(messages));
    if let Some(v) = req.max_tokens { body.insert("max_tokens".into(), json!(v)); }
    if let Some(v) = req.temperature { body.insert("temperature".into(), json!(v)); }
    if let Some(v) = req.top_p { body.insert("top_p".into(), json!(v)); }
    body.insert("stream".into(), json!(req.stream));
    if let Some(s) = &req.stop { body.insert("stop".into(), json!(s)); }
    if let Some(u) = &req.user { body.insert("user".into(), json!(u)); }
    if let Some(tools) = &req.tools {
        let arr: Vec<Value> = tools.iter().map(|t| json!({
            "type": "function",
            "function": { "name": t.name, "description": t.description, "parameters": t.parameters }
        })).collect();
        body.insert("tools".into(), Value::Array(arr));
    }
    if let Some(tc) = &req.tool_choice { body.insert("tool_choice".into(), tc.clone()); }
    for (k, v) in &req.extra { body.insert(k.clone(), v.clone()); }
    Value::Object(body)
}

fn parts_to_openai(parts: &[&ContentPart]) -> Vec<Value> {
    parts.iter().map(|p| match p {
        ContentPart::Text { text } => json!({ "type": "text", "text": text }),
        ContentPart::Image { url, detail } => {
            let mut img = json!({ "type": "image_url", "image_url": { "url": url } });
            if let Some(d) = detail { img["image_url"]["detail"] = json!(d); }
            img
        }
        _ => json!({}),
    }).collect()
}

/// canonical → Anthropic 请求体。
pub fn to_anthropic(req: &CanonicalRequest) -> Value {
    let messages: Vec<Value> = req.messages.iter().map(|m| {
        let content: Vec<Value> = m.content.iter().map(|p| match p {
            ContentPart::Text { text } => json!({ "type": "text", "text": text }),
            ContentPart::Image { url, .. } => json!({ "type": "image", "source": { "type": "url", "url": url } }),
            ContentPart::ToolUse { id, name, input } => json!({ "type": "tool_use", "id": id, "name": name, "input": input }),
            ContentPart::ToolResult { id, content, is_error } => json!({ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error }),
        }).collect();
        json!({ "role": m.role, "content": content })
    }).collect();

    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("max_tokens".into(), json!(req.max_tokens.unwrap_or(4096)));
    if let Some(sys) = &req.system { body.insert("system".into(), json!(sys)); }
    body.insert("messages".into(), Value::Array(messages));
    if let Some(v) = req.temperature { body.insert("temperature".into(), json!(v)); }
    if let Some(v) = req.top_p { body.insert("top_p".into(), json!(v)); }
    body.insert("stream".into(), json!(req.stream));
    if let Some(s) = &req.stop { body.insert("stop_sequences".into(), json!(s)); }
    if let Some(tools) = &req.tools {
        let arr: Vec<Value> = tools.iter().map(|t| json!({
            "name": t.name, "description": t.description, "input_schema": t.parameters
        })).collect();
        body.insert("tools".into(), Value::Array(arr));
    }
    if let Some(tc) = &req.tool_choice { body.insert("tool_choice".into(), tc.clone()); }
    Value::Object(body)
}

/// 上游 CanonicalResponse → 按客户端入站格式序列化。
pub fn to_client(resp: &CanonicalResponse, fmt: InboundFormat) -> Value {
    let usage = resp.usage.clone().unwrap_or_default();
    match fmt {
        InboundFormat::OpenAi | InboundFormat::Unknown => json!({
            "id": resp.id, "object": "chat.completion", "model": resp.model,
            "choices": [{ "index": 0, "message": { "role": "assistant", "content": resp.content },
                          "finish_reason": resp.finish_reason }],
            "usage": { "prompt_tokens": usage.prompt_tokens, "completion_tokens": usage.completion_tokens, "total_tokens": usage.total_tokens }
        }),
        InboundFormat::Anthropic => json!({
            "id": resp.id, "type": "message", "role": "assistant", "model": resp.model,
            "content": [{ "type": "text", "text": resp.content }],
            "stop_reason": resp.finish_reason,
            "usage": { "input_tokens": usage.prompt_tokens, "output_tokens": usage.completion_tokens }
        }),
        InboundFormat::Gemini => json!({
            "candidates": [{ "content": { "parts": [{ "text": resp.content }] } }],
            "usageMetadata": { "promptTokenCount": usage.prompt_tokens, "candidatesTokenCount": usage.completion_tokens, "totalTokenCount": usage.total_tokens }
        }),
    }
}
