# 05 AUTOFIT 适配模块（relay-autofit）

> 功能对标：任何客户端格式都能接，任何上游渠道格式都能出。接口契约见 14 文档，SSE 规范见 14-C。

## 1. 职责与位置

```
relay-api ──原始请求──▶ relay-autofit.detect_inbound() ──CanonicalRequest──▶ relay-gateway
relay-gateway ──CanonicalRequest + 渠道type──▶ relay-autofit.to_provider() ──上游原生──▶ relay-provider
上游响应 ──▶ relay-autofit.to_client() ──按入站格式返回──▶ 客户端
```

## 2. 入站识别 detect_inbound

```rust
// crates/relay-autofit/src/detect.rs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InboundFormat { OpenAi, Anthropic, Gemini, Unknown }

pub fn detect_inbound(path: &str, headers: &HeaderMap, body: &Value) -> InboundFormat {
    match path {
        "/v1/chat/completions" => InboundFormat::OpenAi,
        "/v1/messages" => InboundFormat::Anthropic,
        p if p.starts_with("/v1beta/models/") && p.ends_with(":generateContent") => InboundFormat::Gemini,
        _ => {
            if body.get("messages").is_some()
               && (body.get("max_tokens").is_some() || headers.contains_key("anthropic-version")) {
                InboundFormat::Anthropic
            } else if body.get("messages").is_some() { InboundFormat::OpenAi }
            else if body.get("contents").is_some() { InboundFormat::Gemini }
            else { InboundFormat::Unknown }
        }
    }
}
```

## 3. 入站解析（OpenAI → Canonical）

```rust
pub fn parse_openai(body: &Value) -> Result<CanonicalRequest, AppError> {
    let model = str_field(body, "model")?;
    let messages = body.get("messages").and_then(Value::as_array)
        .ok_or_else(|| AppError::bad_request("messages 缺失"))?;
    if messages.is_empty() || messages.len() > 512 { return Err(AppError::bad_request("messages 数量 1-512")); }
    let mut canonical = vec![];
    for m in messages {
        let role = str_field(m, "role")?;
        match m.get("content") {
            Some(Value::String(s)) => canonical.push(CanonicalMessage { role, content: vec![ContentPart::Text { text: s.clone() }], name: None }),
            Some(Value::Array(parts)) => {
                let mut content = vec![];
                for p in parts {
                    match p.get("type").and_then(Value::as_str) {
                        Some("text") => content.push(ContentPart::Text { text: str_field(p, "text")? }),
                        Some("image_url") => {
                            let url = p.pointer("/image_url/url").and_then(Value::as_str)
                                .ok_or_else(|| AppError::bad_request("image_url.url 缺失"))?.to_string();
                            let detail = p.pointer("/image_url/detail").and_then(Value::as_str).map(String::from);
                            content.push(ContentPart::Image { url, detail });
                        }
                        Some(other) => return Err(AppError::bad_request(format!("不支持的 content 类型: {other}"))),
                        None => return Err(AppError::bad_request("content 数组元素缺 type")),
                    }
                }
                canonical.push(CanonicalMessage { role, content, name: None });
            }
            _ => return Err(AppError::bad_request("content 必须为字符串或数组")),
        }
    }
    // 工具调用：assistant.tool_calls → ToolUse；role=tool → ToolResult
    // ...
    Ok(CanonicalRequest {
        model,
        messages: canonical,
        system: None,   // OpenAI 无顶层 system，首个 system 消息保留在 messages
        max_tokens: body.get("max_tokens").and_then(Value::as_u64).map(|v| v as u32),
        temperature: body.get("temperature").and_then(Value::as_f64).map(|v| v as f32),
        top_p: body.get("top_p").and_then(Value::as_f64).map(|v| v as f32),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: parse_tools(body.get("tools")),
        tool_choice: body.get("tool_choice").cloned(),
        stop: body.get("stop").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
        user: body.get("user").and_then(Value::as_str).map(String::from),
        extra: extra_fields(body, &["model","messages","max_tokens","temperature","top_p","stream","tools","tool_choice","stop","user"]),
    })
}
```

## 4. 入站解析（A社 → Canonical）

```rust
pub fn parse_anthropic(body: &Value) -> Result<CanonicalRequest, AppError> {
    let model = str_field(body, "model")?;
    let max_tokens = body.get("max_tokens").and_then(Value::as_u64)
        .ok_or_else(|| AppError::bad_request("A社格式必须带 max_tokens"))? as u32;
    let system = body.get("system").and_then(Value::as_str).map(String::from);
    let messages = body.get("messages").and_then(Value::as_array)
        .ok_or_else(|| AppError::bad_request("messages 缺失"))?;
    // content 数组元素 type: text / image / tool_use / tool_result
    // image.source: {type:"base64"|"url", media_type, data|url}
    // tool_use: {id, name, input}；tool_result: {tool_use_id, content, is_error}
    // → 映射 ContentPart 各变体
    // 注意：A社 user 消息不允许带 system 角色，system 已在顶层
    // stop_sequences → stop；tools → CanonicalTool（input_schema → parameters）
    // 未知字段 → extra
}
```

## 5. 出站转换 to_provider

```rust
pub fn to_provider(req: &CanonicalRequest, provider_type: &str) -> Result<Value, AppError> {
    match provider_type {
        "openai" | "deepseek" | "qwen" | "zhipu" | ... => Ok(to_openai(req)),   // OpenAI 兼容全家
        "anthropic" => Ok(to_anthropic(req)),
        "gemini" => Ok(to_gemini(req)),
        "azure" => { /* 同 openai + deployment 名替换 model */ }
        _ => Err(AppError::bad_request(format!("未知渠道类型 {provider_type}"))),
    }
}
```

**to_openai 要点**：`system` → 合成 `{role:"system"}` 消息（放最前）；图片 → `image_url`；`ToolUse` → `tool_calls`；`ToolResult` → `{role:"tool",tool_call_id}`。

**to_anthropic 要点**：
```json
{
  "model": "...",
  "max_tokens": 4096,
  "system": "…",                    // canonical.system 回写
  "messages": [{ "role": "user", "content": [{ "type": "text", "text": "…" }] }],
  "tools": [{ "name": "…", "description": "…", "input_schema": {…} }],
  "stop_sequences": ["…"]
}
```
图片 → `{"type":"image","source":{"type":"url","url":"…"}}`；工具回环：assistant `tool_calls` → `tool_use` 块，后续 `tool` 消息 → `tool_result` 块（user 角色）。

**to_gemini 要点**：`contents:[{role:"user"/"model", parts:[{text} | {inline_data}|{function_call}|{function_response}]}]`；`generationConfig:{maxOutputTokens,temperature,stopSequences}`；`tools:[{functionDeclarations:[…]}]`。

## 6. 响应转换 to_client

```rust
// 上游 CanonicalResponse → 按客户端入站格式序列化
pub fn to_client(resp: &CanonicalResponse, fmt: InboundFormat) -> Value {
    match fmt {
        InboundFormat::OpenAi => json!({
            "id": resp.id, "object": "chat.completion", "model": resp.model,
            "choices": [{ "index": 0, "message": { "role": "assistant", "content": resp.content },
                          "finish_reason": resp.finish_reason }],
            "usage": { "prompt_tokens": u, "completion_tokens": u, "total_tokens": u }
        }),
        InboundFormat::Anthropic => json!({
            "id": resp.id, "type": "message", "role": "assistant", "model": resp.model,
            "content": [{ "type": "text", "text": resp.content }],
            "stop_reason": resp.finish_reason, "usage": { "input_tokens": u, "output_tokens": u }
        }),
        InboundFormat::Gemini => json!({ "candidates": [{ "content": { "parts": [{ "text": resp.content }] } }],
                                         "usageMetadata": { … } }),
        InboundFormat::Unknown => json!(resp),
    }
}
```

## 7. 流式转换

```rust
// OpenAI 上游 → A社客户端：逐事件映射（表见 14-C4）
pub fn openai_chunk_to_anthropic(ev: &Value) -> Option<String>;   // 返回 SSE data 行
// A社上游 → OpenAI 客户端
pub fn anthropic_event_to_openai(ev: &Value) -> Option<String>;
// 终止归一：OpenAI [DONE] ⇄ A社 message_stop
```

## 8. 错误归一

```rust
// 上游 ProviderError → 客户端格式错误体
// OpenAI: {"error":{"message":…,"type":"upstream_error","code":…}}
// A社:    {"type":"error","error":{"type":"api_error","message":…}}
```

## 9. 本模块测试要求

- [ ] detect_inbound 端点 + 字段嗅探全分支
- [ ] parse_openai：文本/多图/工具调用/未知字段进 extra
- [ ] parse_anthropic：system/max_tokens 必填/图片/tool_use↔tool_result
- [ ] to_provider 三格式往返（canonical → openai → 再解析 = 原 canonical）
- [ ] SSE 双向转换逐事件单测
- [ ] 错误归一格式断言
