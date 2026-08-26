//! Anthropic 原生 Provider（POST /v1/messages，x-api-key 认证）。
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use relay_core::{AppError, CanonicalRequest, CanonicalResponse, Channel, StreamEvent, TokenUsage};
use serde_json::{json, Value};

use crate::provider_http;
use crate::{decrypt_key, join_url, Provider};

pub struct AnthropicProvider {
    pub master_key: [u8; 32],
}

impl AnthropicProvider {
    pub fn new(master_key: [u8; 32]) -> Self {
        Self { master_key }
    }
    fn client(&self, chan: &Channel) -> Result<reqwest::Client, AppError> {
        reqwest::Client::builder()
            .timeout(Duration::from_millis(chan.timeout_ms.max(1000) as u64))
            .build()
            .map_err(|e| AppError::internal(format!("HTTP 客户端构建失败: {e}")))
    }
    fn url(&self, chan: &Channel) -> String {
        join_url(&chan.base_url, "/v1/messages")
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider {
    fn type_name(&self) -> &str { "anthropic" }

    async fn chat(&self, _req: &CanonicalRequest, body: Value, chan: &Channel) -> Result<CanonicalResponse, AppError> {
        let key = decrypt_key(chan, &self.master_key)?;
        let url = self.url(chan);
        let client = self.client(chan)?;
        let resp = client.post(&url)
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send().await
            .map_err(|e| AppError::provider(502, format!("上游请求失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let val: Value = serde_json::from_str(&text).map_err(|e| AppError::provider(status, format!("上游响应非 JSON: {e}")))?;
        if !(200..300).contains(&status) {
            let msg = val.pointer("/error/message").and_then(Value::as_str).unwrap_or(&text).to_string();
            return Err(AppError::provider(status, msg));
        }
        Ok(parse_anthropic_response(&val))
    }

    async fn chat_stream(&self, _req: &CanonicalRequest, body: Value, chan: &Channel) -> Result<BoxStream<'static, Result<StreamEvent, AppError>>, AppError> {
        let key = decrypt_key(chan, &self.master_key)?;
        let url = self.url(chan);
        let client = self.client(chan)?;
        let mut send_body = body;
        if let Some(obj) = send_body.as_object_mut() { obj.insert("stream".into(), json!(true)); }
        let resp = client.post(&url).header("x-api-key", &key).header("anthropic-version", "2023-06-01").json(&send_body).send().await
            .map_err(|e| AppError::provider(502, format!("上游流式请求失败: {e}")))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::provider(status, text));
        }
        let byte_stream = resp.bytes_stream();
        let stream = provider_http::sse_lines(byte_stream).filter_map(|line| async move {
            match line {
                Ok(l) => parse_anthropic_event(&l),
                Err(e) => Some(Err(AppError::provider(502, format!("流读取失败: {e}")))),
            }
        });
        Ok(stream.boxed())
    }

    async fn list_models(&self, chan: &Channel) -> Result<Vec<String>, AppError> {
        let key = decrypt_key(chan, &self.master_key)?;
        let url = join_url(&chan.base_url, "/v1/models");
        let client = self.client(chan)?;
        let resp = client.get(&url).header("x-api-key", &key).header("anthropic-version", "2023-06-01").send().await
            .map_err(|e| AppError::provider(502, format!("探测失败: {e}")))?;
        let status = resp.status().as_u16();
        let val: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if !(200..300).contains(&status) {
            // 退化为渠道配置的模型列表
            return Ok(chan.models.split(',').filter(|s| !s.trim().is_empty()).map(|s| s.trim().to_string()).collect());
        }
        Ok(val.pointer("/data").and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from)).collect())
            .unwrap_or_default())
    }
}

/// 解析 Anthropic 非流式响应 → CanonicalResponse。
pub fn parse_anthropic_response(val: &Value) -> CanonicalResponse {
    let id = val.pointer("/id").and_then(Value::as_str).unwrap_or("").to_string();
    let model = val.pointer("/model").and_then(Value::as_str).unwrap_or("").to_string();
    let content = val.pointer("/content").and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|b| {
            if b.get("type").and_then(Value::as_str) == Some("text") {
                b.get("text").and_then(Value::as_str).map(String::from)
            } else { None }
        }).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    let finish_reason = val.pointer("/stop_reason").and_then(Value::as_str).map(String::from);
    let usage = val.pointer("/usage").map(|u| TokenUsage {
        prompt_tokens: u.pointer("/input_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
        completion_tokens: u.pointer("/output_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
        total_tokens: u.pointer("/input_tokens").and_then(Value::as_u64).unwrap_or(0) as u32
            + u.pointer("/output_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
    });
    CanonicalResponse { id, model, content, finish_reason, usage }
}

/// 解析 Anthropic SSE 一行（data: {...}）→ Option<StreamEvent>。
fn parse_anthropic_event(line: &str) -> Option<Result<StreamEvent, AppError>> {
    let line = line.trim();
    if line.starts_with("event:") || line.is_empty() {
        return None;
    }
    let data = line.strip_prefix("data:")?.trim();
    let val: Value = serde_json::from_str(data).ok()?;
    match val.pointer("/type").and_then(Value::as_str) {
        Some("content_block_delta") => {
            if let Some(text) = val.pointer("/delta/text").and_then(Value::as_str) {
                if !text.is_empty() { return Some(Ok(StreamEvent::TextDelta { text: text.into() })); }
            }
            None
        }
        Some("message_delta") => {
            if let Some(u) = val.pointer("/usage") {
                let out = u.pointer("/output_tokens").and_then(Value::as_u64).unwrap_or(0) as u32;
                return Some(Ok(StreamEvent::Usage { usage: TokenUsage { prompt_tokens: 0, completion_tokens: out, total_tokens: out } }));
            }
            None
        }
        Some("message_stop") => Some(Ok(StreamEvent::Done)),
        _ => None,
    }
}
