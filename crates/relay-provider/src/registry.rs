//! Provider 注册表：type -> Box<dyn Provider>。
use std::collections::HashMap;
use std::sync::Arc;

use relay_core::{AppError, Channel};

use crate::anthropic::AnthropicProvider;
use crate::openai::OpenAiCompatProvider;
use crate::Provider;

pub struct ProviderRegistry {
    map: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    /// 注册全部内置 Provider。
    pub fn new(master_key: [u8; 32]) -> Self {
        let mut map: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        // OpenAI 兼容全家
        for t in [
            "openai", "deepseek", "qwen", "zhipu", "kimi", "minimax", "volcengine", "qianfan",
            "hunyuan", "groq", "together", "mistral", "xai", "openrouter", "siliconflow", "ollama",
            "azure",
        ] {
            map.insert(t.into(), Arc::new(OpenAiCompatProvider::new(t, master_key)));
        }
        map.insert("anthropic".into(), Arc::new(AnthropicProvider::new(master_key)));
        Self { map }
    }

    pub fn get(&self, type_name: &str) -> Result<Arc<dyn Provider>, AppError> {
        self.map
            .get(type_name)
            .cloned()
            .ok_or_else(|| AppError::bad_request(format!("未知渠道类型: {type_name}")))
    }

    /// 是否为 OpenAI 兼容类型（autofit 用）。
    pub fn is_openai_compat(type_name: &str) -> bool {
        matches!(
            type_name,
            "openai" | "deepseek" | "qwen" | "zhipu" | "kimi" | "minimax" | "volcengine"
                | "qianfan" | "hunyuan" | "groq" | "together" | "mistral" | "xai" | "openrouter"
                | "siliconflow" | "ollama" | "azure"
        )
    }

    /// 所有可用渠道（按 group 过滤 + 模型匹配）。
    pub fn usable_channels(&self, channels: &[Channel], group_id: i64, model: &str) -> Vec<Channel> {
        channels
            .iter()
            .filter(|c| {
                if c.status != 1 {
                    return false;
                }
                // 分组授权
                let gids = relay_core::util::parse_group_ids(&c.group_ids);
                if !gids.contains(&group_id) {
                    return false;
                }
                // 模型匹配（空 = 全部）
                if c.models.trim().is_empty() {
                    return true;
                }
                let mapping = parse_model_mapping(&c.model_mapping);
                let target = mapping.get(model).map(|s| s.as_str()).unwrap_or(model);
                c.models.split(',').any(|m| m.trim() == target || m.trim() == model)
            })
            .cloned()
            .collect()
    }
}

/// 解析 model_mapping JSON。
pub fn parse_model_mapping(s: &str) -> std::collections::HashMap<String, String> {
    if s.is_empty() {
        return HashMap::new();
    }
    serde_json::from_str(s).unwrap_or_default()
}

/// 客户端模型 → 渠道模型映射（命中用映射值，未命中原样透传）。
pub fn map_model(chan: &Channel, model: &str) -> String {
    parse_model_mapping(&chan.model_mapping)
        .get(model)
        .cloned()
        .unwrap_or_else(|| model.to_string())
}
