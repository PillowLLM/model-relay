//! relay-provider: Provider trait + 内置渠道 + 注册表 + 渠道选择。
pub mod openai;
pub mod anthropic;
pub mod registry;
pub mod selector;
pub mod provider_http;

use futures::stream::BoxStream;
use relay_core::{AppError, CanonicalRequest, CanonicalResponse, Channel, StreamEvent};
use serde_json::Value;

pub use registry::{map_model, ProviderRegistry};
pub use selector::ChannelSelector;

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn type_name(&self) -> &str;
    /// 非流式对话。body 为 autofit.to_provider() 转换后的上游原生请求体。
    async fn chat(&self, req: &CanonicalRequest, body: Value, chan: &Channel) -> Result<CanonicalResponse, AppError>;
    /// 流式对话，返回 canonical StreamEvent 流。
    async fn chat_stream(&self, req: &CanonicalRequest, body: Value, chan: &Channel)
        -> Result<BoxStream<'static, Result<StreamEvent, AppError>>, AppError>;
    /// 健康探测：列出上游模型。
    async fn list_models(&self, chan: &Channel) -> Result<Vec<String>, AppError>;
}

/// 解密渠道 API Key。
pub fn decrypt_key(chan: &Channel, master_key: &[u8; 32]) -> Result<String, AppError> {
    relay_core::crypto::decrypt(master_key, &chan.api_key_enc)
}

/// 拼接上游 URL：base_url 末尾若含 /v1 则直接接 /chat/completions，否则补 /v1。
pub fn join_url(base_url: &str, suffix: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}{suffix}")
    } else if suffix.starts_with("/v1") {
        format!("{base}{suffix}")
    } else {
        format!("{base}/v1{suffix}")
    }
}
