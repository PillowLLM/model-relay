use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::error::AppError;

/// AUTOFIT 统一中间格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRequest {
    pub model: String,
    pub messages: Vec<CanonicalMessage>,
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stream: bool,
    pub tools: Option<Vec<CanonicalTool>>,
    pub tool_choice: Option<Value>,
    pub stop: Option<Vec<String>>,
    pub user: Option<String>,
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMessage {
    pub role: String,
    pub content: Vec<ContentPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { url: String, #[serde(default, skip_serializing_if = "Option::is_none")] detail: Option<String> },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, content: String, #[serde(default)] is_error: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 流式块（AUTOFIT 转换后的统一事件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    TextDelta { text: String },
    ToolUseDelta { index: usize, name: Option<String>, input_delta: String },
    ToolResultDelta { index: usize, content: String },
    Usage { usage: TokenUsage },
    Done,
}

pub fn str_field(obj: &Value, key: &str) -> Result<String, AppError> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| AppError::bad_request(format!("字段 {key} 缺失或非字符串")))
}

/// 收集未知字段进 extra（排除已知键）。
pub fn extra_fields(obj: &Value, known: &[&str]) -> HashMap<String, Value> {
    let mut extra = HashMap::new();
    if let Some(map) = obj.as_object() {
        for (k, v) in map {
            if !known.contains(&k.as_str()) {
                extra.insert(k.clone(), v.clone());
            }
        }
    }
    extra
}
