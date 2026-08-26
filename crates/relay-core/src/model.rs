use serde::{Deserialize, Serialize};

/// 业务实体（与 01 文档表字段一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub email: String,
    pub phone: String,
    pub role: String, // admin / operator / user
    pub group_id: i64,
    pub quota_limit: i64, // -1 不限
    pub used_quota: i64,
    pub status: i32, // 1 启用 / 0 禁用
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub quota_multiplier: f64,
    pub remark: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub base_url: String,
    pub api_key_enc: String,
    pub models: String, // 逗号分隔；空 = 全部
    pub model_mapping: String,
    pub priority: i32,
    pub weight: i32,
    pub group_ids: String, // "1,2"
    pub status: i32,
    pub auto_ban: i32,
    pub max_retries: i32,
    pub timeout_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub key_hash: String,
    pub status: i32,
    pub quota_limit: i64,
    pub used_quota: i64,
    pub expired_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemCode {
    pub id: i64,
    pub code: String,
    pub quota: i64,
    pub status: i32, // 0 未用 / 1 已用 / 2 禁用
    pub used_by: Option<i64>,
    pub used_at: Option<String>,
    pub expires_at: Option<String>,
    pub remark: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLog {
    pub id: i64,
    pub token_id: i64,
    pub user_id: i64,
    pub group_id: i64,
    pub channel_id: i64,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cost: f64,
    pub latency_ms: i64,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    pub id: i64,
    pub model: String,
    pub prompt_price: f64,
    pub completion_price: f64,
    pub created_at: String,
}

impl Default for Pricing {
    fn default() -> Self {
        Self {
            id: 0,
            model: String::new(),
            prompt_price: 0.0,
            completion_price: 0.0,
            created_at: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizerFile {
    pub id: i64,
    pub family: String,
    pub filename: String,
    pub data: Vec<u8>,
    pub checksum: String,
    pub created_at: String,
}

/// 用量查询过滤。
#[derive(Debug, Clone, Default)]
pub struct UsageFilter {
    pub user_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub model: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatSummary {
    pub total_tokens: i64,
    pub total_cost: f64,
    pub requests: i64,
    pub by_model: Vec<ByGroup>,
    pub by_day: Vec<ByGroup>,
    pub by_channel: Vec<ByGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ByGroup {
    pub key: String,
    pub tokens: i64,
    pub cost: f64,
}

/// 列表统一返回。
#[derive(Debug, Clone, Serialize)]
pub struct Page<T: Serialize> {
    pub items: Vec<T>,
    pub total: u32,
}
