//! OpenAI 兼容 Provider（覆盖 openai/deepseek/qwen/zhipu/kimi/groq/...）。
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use relay_core::{AppError, CanonicalRequest, CanonicalResponse, Channel, StreamEvent, TokenUsage};
use serde_json::{json, Value};

use super::provider_http;
use crate::{decrypt_key, join_url, Provider};

pub struct OpenAiCompatProvider {
    pub type_name: String,
    pub master_key: [u8; 32],
}

impl OpenAiCompatProvider {
    pub fn new(type_name: impl Into<String>, master_key: [u8; 32]) -> Self {
        Self { type_name: type_name.into(), master_key }
    }

    fn client(&self, chan: &Channel) -> Result<reqwest::Client, AppError> {
        reqwest::Client::builder()
            .timeout(Duration::from_millis(chan.timeout_ms.max(1000) as u64))
            .build()
            .map_err(|e| AppError::internal(format!("HTTP 客户端构建失败: {e}")))
    }

    fn url(&self, chan: &Channel) -> String {
        join_url(&chan.base_url, "/chat/completions")
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatProvider {
    fn type_name(&self) -> &str {
        &self.type_name
    }

    async fn chat(&self, _req: &CanonicalRequest, body: Value, chan: &Channel) -> Result<CanonicalResponse, AppError> {
        let key = decrypt_key(chan, &self.master_key)?;
        let url = self.url(chan);
        let client = self.client(chan)?;
        let resp = client
            .post(&url)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::provider(502, format!("上游请求失败: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| AppError::provider(502, format!("读取上游响应失败: {e}")))?;
        let val: Value = serde_json::from_str(&text)
            .map_err(|e| AppError::provider(status, format!("上游响应非 JSON: {e}")))?;
        if !(200..300).contains(&status) {
            let msg = val.pointer("/error/message").and_then(Value::as_str).unwrap_or(&text).to_string();
            return Err(AppError::provider(status, msg));
        }
        Ok(parse_openai_response(&val))
    }

    async fn chat_stream(&self, _req: &CanonicalRequest, body: Value, chan: &Channel) -> Result<BoxStream<'static, Result<StreamEvent, AppError>>, AppError> {
        let key = decrypt_key(chan, &self.master_key)?;
        let url = self.url(chan);
        let client = self.client(chan)?;
        let mut send_body = body;
        if let Some(obj) = send_body.as_object_mut() {
            obj.insert("stream".into(), json!(true));
        }
        let resp = client.post(&url).bearer_auth(&key).json(&send_body).send().await
            .map_err(|e| AppError::provider(502, format!("上游流式请求失败: {e}")))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::provider(status, text));
        }
        let byte_stream = resp.bytes_stream();
        let stream = provider_http::sse_lines(byte_stream).filter_map(|line| async move {
            match line {
                Ok(l) => parse_openai_chunk(&l),
                Err(e) => Some(Err(AppError::provider(502, format!("流读取失败: {e}")))),
            }
        });
        Ok(stream.boxed())
    }

    async fn list_models(&self, chan: &Channel) -> Result<Vec<String>, AppError> {
        let key = decrypt_key(chan, &self.master_key)?;
        let url = join_url(&chan.base_url, "/models");
        let client = self.client(chan)?;
        let resp = client.get(&url).bearer_auth(&key).send().await
            .map_err(|e| AppError::provider(502, format!("探测失败: {e}")))?;
        let status = resp.status().as_u16();
        let val: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if !(200..300).contains(&status) {
            return Err(AppError::provider(status, "list_models 失败"));
        }
        Ok(val.pointer("/data")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from)).collect())
            .unwrap_or_default())
    }
}

/// 解析 OpenAI 非流式响应 → CanonicalResponse。
pub fn parse_openai_response(val: &Value) -> CanonicalResponse {
    let id = val.pointer("/id").and_then(Value::as_str).unwrap_or("").to_string();
    let model = val.pointer("/model").and_then(Value::as_str).unwrap_or("").to_string();
    let content = val
        .pointer("/choices/0/message/content")
        .map(|c| match c {
            Value::String(s) => s.clone(),
            _ => c.to_string(),
        })
        .unwrap_or_default();
    let finish_reason = val.pointer("/choices/0/finish_reason").and_then(Value::as_str).map(String::from);
    let usage = val.pointer("/usage").map(|u| TokenUsage {
        prompt_tokens: u.pointer("/prompt_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
        completion_tokens: u.pointer("/completion_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
        total_tokens: u.pointer("/total_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
    });
    CanonicalResponse { id, model, content, finish_reason, usage }
}

/// 解析 OpenAI 流式 chunk 一行 → Option<StreamEvent>。
fn parse_openai_chunk(line: &str) -> Option<Result<StreamEvent, AppError>> {
    let line = line.trim();
    if !line.starts_with("data:") {
        return None;
    }
    let data = line.trim_start_matches("data:").trim();
    if data == "[DONE]" {
        return Some(Ok(StreamEvent::Done));
    }
    let val: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return None,
    };
    // usage（部分上游在终止块带 usage）
    if let Some(u) = val.pointer("/usage") {
        let usage = TokenUsage {
            prompt_tokens: u.pointer("/prompt_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
            completion_tokens: u.pointer("/completion_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
            total_tokens: u.pointer("/total_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
        };
        return Some(Ok(StreamEvent::Usage { usage }));
    }
    if let Some(text) = val.pointer("/choices/0/delta/content").and_then(Value::as_str) {
        if !text.is_empty() {
            return Some(Ok(StreamEvent::TextDelta { text: text.into() }));
        }
    }
    None
}
