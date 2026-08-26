//! 入站解析：OpenAI / Anthropic → CanonicalRequest。
use relay_core::error::AppError;
use relay_core::*;
use serde_json::Value;

use relay_core::{extra_fields, str_field};

/// OpenAI body → CanonicalRequest。
pub fn parse_openai(body: &Value) -> Result<CanonicalRequest, AppError> {
    let model = str_field(body, "model")?;
    if model.len() > 64 {
        return Err(AppError::bad_request("model 长度超限"));
    }
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::bad_request("messages 缺失"))?;
    if messages.is_empty() || messages.len() > 512 {
        return Err(AppError::bad_request("messages 数量需 1-512"));
    }
    let mut canonical = Vec::with_capacity(messages.len());
    let mut system: Option<String> = None;
    for m in messages {
        let role = str_field(m, "role")?;
        // system 角色提取为顶层 system（保留首条）
        if role == "system" {
            if system.is_none() {
                system = Some(text_of(m));
            }
            // 仍保留在 messages 中（OpenAI 习惯），不丢弃
            canonical.push(CanonicalMessage {
                role,
                content: vec![ContentPart::Text { text: text_of(m) }],
                name: None,
            });
            continue;
        }
        let content = parse_content(m.get("content"))?;
        // assistant tool_calls → ToolUse
        if role == "assistant" {
            if let Some(calls) = m.get("tool_calls").and_then(Value::as_array) {
                let mut parts = content;
                for c in calls {
                    let id = c.pointer("/id").and_then(Value::as_str).unwrap_or("").to_string();
                    let name = c.pointer("/function/name").and_then(Value::as_str).unwrap_or("").to_string();
                    let args_str = c.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}");
                    let input: Value = serde_json::from_str(args_str).unwrap_or(Value::Null);
                    parts.push(ContentPart::ToolUse { id, name, input });
                }
                canonical.push(CanonicalMessage { role, content: parts, name: None });
                continue;
            }
        }
        if role == "tool" {
            let id = m.get("tool_call_id").and_then(Value::as_str).unwrap_or("").to_string();
            let text = m.get("content").and_then(Value::as_str).unwrap_or("").to_string();
            canonical.push(CanonicalMessage {
                role: "user".into(),
                content: vec![ContentPart::ToolResult { id, content: text, is_error: false }],
                name: None,
            });
            continue;
        }
        canonical.push(CanonicalMessage { role, content, name: None });
    }

    Ok(CanonicalRequest {
        model,
        messages: canonical,
        system,
        max_tokens: body.get("max_tokens").and_then(Value::as_u64).map(|v| v as u32),
        temperature: body.get("temperature").and_then(Value::as_f64).map(|v| v as f32),
        top_p: body.get("top_p").and_then(Value::as_f64).map(|v| v as f32),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: parse_tools(body.get("tools")),
        tool_choice: body.get("tool_choice").cloned(),
        stop: body.get("stop").and_then(Value::as_array).map(|a| {
            a.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        }),
        user: body.get("user").and_then(Value::as_str).map(String::from),
        extra: extra_fields(body, &["model", "messages", "max_tokens", "temperature", "top_p", "stream", "tools", "tool_choice", "stop", "user"]),
    })
}

/// Anthropic body → CanonicalRequest。
pub fn parse_anthropic(body: &Value) -> Result<CanonicalRequest, AppError> {
    let model = str_field(body, "model")?;
    let max_tokens = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::bad_request("A社格式必须带 max_tokens"))? as u32;
    let system = body.get("system").and_then(Value::as_str).map(String::from);
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::bad_request("messages 缺失"))?;
    if messages.is_empty() || messages.len() > 512 {
        return Err(AppError::bad_request("messages 数量需 1-512"));
    }
    let mut canonical = Vec::with_capacity(messages.len());
    for m in messages {
        let role = str_field(m, "role")?;
        let mut content = Vec::new();
        if let Some(arr) = m.get("content").and_then(Value::as_array) {
            for p in arr {
                match p.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = p.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                        content.push(ContentPart::Text { text });
                    }
                    Some("image") => {
                        let url = p.pointer("/source/url").and_then(Value::as_str).unwrap_or("").to_string();
                        content.push(ContentPart::Image { url, detail: None });
                    }
                    Some("tool_use") => {
                        let id = p.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        let name = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        let input = p.get("input").cloned().unwrap_or(Value::Null);
                        content.push(ContentPart::ToolUse { id, name, input });
                    }
                    Some("tool_result") => {
                        let id = p.get("tool_use_id").and_then(Value::as_str).unwrap_or("").to_string();
                        let text = p.get("content").and_then(Value::as_str).unwrap_or("").to_string();
                        let is_error = p.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                        content.push(ContentPart::ToolResult { id, content: text, is_error });
                    }
                    _ => {}
                }
            }
        } else if let Some(s) = m.get("content").and_then(Value::as_str) {
            content.push(ContentPart::Text { text: s.to_string() });
        }
        canonical.push(CanonicalMessage { role, content, name: None });
    }

    Ok(CanonicalRequest {
        model,
        messages: canonical,
        system,
        max_tokens: Some(max_tokens),
        temperature: body.get("temperature").and_then(Value::as_f64).map(|v| v as f32),
        top_p: body.get("top_p").and_then(Value::as_f64).map(|v| v as f32),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: parse_tools_anthropic(body.get("tools")),
        tool_choice: body.get("tool_choice").cloned(),
        stop: body.get("stop_sequences").and_then(Value::as_array).map(|a| {
            a.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        }),
        user: None,
        extra: extra_fields(body, &["model", "max_tokens", "system", "messages", "temperature", "top_p", "stream", "tools", "tool_choice", "stop_sequences"]),
    })
}

fn text_of(m: &Value) -> String {
    match m.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| {
                if p.get("type").and_then(Value::as_str) == Some("text") {
                    p.get("text").and_then(Value::as_str).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn parse_content(c: Option<&Value>) -> Result<Vec<ContentPart>, AppError> {
    match c {
        Some(Value::String(s)) => Ok(vec![ContentPart::Text { text: s.clone() }]),
        Some(Value::Array(parts)) => {
            let mut content = Vec::new();
            for p in parts {
                match p.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = p.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                        content.push(ContentPart::Text { text });
                    }
                    Some("image_url") => {
                        let url = p
                            .pointer("/image_url/url")
                            .and_then(Value::as_str)
                            .ok_or_else(|| AppError::bad_request("image_url.url 缺失"))?
                            .to_string();
                        let detail = p.pointer("/image_url/detail").and_then(Value::as_str).map(String::from);
                        content.push(ContentPart::Image { url, detail });
                    }
                    _ => {}
                }
            }
            Ok(content)
        }
        _ => Err(AppError::bad_request("content 必须为字符串或数组")),
    }
}

fn parse_tools(v: Option<&Value>) -> Option<Vec<CanonicalTool>> {
    let arr = v?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|t| {
                let f = t.get("function")?;
                Some(CanonicalTool {
                    name: f.get("name")?.as_str()?.to_string(),
                    description: f.get("description").and_then(Value::as_str).map(String::from),
                    parameters: f.get("parameters").cloned().unwrap_or(Value::Null),
                })
            })
            .collect(),
    )
}

fn parse_tools_anthropic(v: Option<&Value>) -> Option<Vec<CanonicalTool>> {
    let arr = v?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|t| Some(CanonicalTool {
                name: t.get("name")?.as_str()?.to_string(),
                description: t.get("description").and_then(Value::as_str).map(String::from),
                parameters: t.get("input_schema").cloned().unwrap_or(Value::Null),
            }))
            .collect(),
    )
}
