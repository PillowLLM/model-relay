//! relay-provider 注册表 / 渠道选择 / 模型映射 测试（钩子/插件机制 + 注入）。
use relay_core::Channel;
use relay_provider::{map_model, ChannelSelector, ProviderRegistry};

fn chan(id: i64, name: &str, models: &str, group_ids: &str, status: i32, weight: i32) -> Channel {
    Channel {
        id,
        name: name.into(),
        r#type: "openai".into(),
        base_url: "https://api.example.com".into(),
        api_key_enc: String::new(),
        models: models.into(),
        model_mapping: String::new(),
        priority: 10,
        weight,
        group_ids: group_ids.into(),
        status,
        auto_ban: 1,
        max_retries: 2,
        timeout_ms: 30000,
        created_at: String::new(),
    }
}

// ---- hook/plugin: provider registry registration & dispatch ----

#[test]
fn hook_registered_providers_resolve_unknown_rejected() {
    let reg = ProviderRegistry::new([7u8; 32]);
    assert!(reg.get("openai").is_ok());
    assert!(reg.get("anthropic").is_ok());
    // unregistered / unknown provider type must be rejected
    assert!(reg.get("hacker-made-up-provider").is_err());
}

#[test]
fn is_openai_compat_classification() {
    assert!(ProviderRegistry::is_openai_compat("deepseek"));
    assert!(ProviderRegistry::is_openai_compat("openai"));
    assert!(!ProviderRegistry::is_openai_compat("anthropic"));
    assert!(!ProviderRegistry::is_openai_compat("nope"));
}

#[test]
fn usable_channels_filters_status_group_and_model() {
    let reg = ProviderRegistry::new([7u8; 32]);
    let channels = vec![
        chan(1, "enabled", "gpt-4o", "1", 1, 1),      // enabled, group 1, model gpt-4o
        chan(2, "disabled", "gpt-4o", "1", 0, 1),     // disabled
        chan(3, "other-group", "gpt-4o", "2", 1, 1),  // group 2 only
        chan(4, "other-model", "claude", "1", 1, 1),  // different model
    ];
    let usable = reg.usable_channels(&channels, 1, "gpt-4o");
    let ids: Vec<i64> = usable.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![1]);
}

#[test]
fn usable_channels_empty_models_means_all_models() {
    let reg = ProviderRegistry::new([7u8; 32]);
    let channels = vec![chan(1, "catch-all", "", "1", 1, 1)];
    let usable = reg.usable_channels(&channels, 1, "any-model-at-all");
    assert_eq!(usable.len(), 1);
}

// ---- hook/plugin: weighted channel selector round-robin ----

#[test]
fn selector_orders_and_dedups() {
    let sel = ChannelSelector::new();
    let candidates = vec![chan(1, "a", "", "1", 1, 1), chan(2, "b", "", "1", 1, 1)];
    let ordered = sel.ordered(&candidates);
    assert_eq!(ordered.len(), 2);
    // all candidates present, no duplicates
    let mut ids: Vec<i64> = ordered.iter().map(|c| c.id).collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2]);
    assert!(sel.ordered(&[]).is_empty());
}

#[test]
fn selector_roundrobin_rotates_start() {
    let sel = ChannelSelector::new();
    let candidates = vec![chan(1, "a", "", "1", 1, 1), chan(2, "b", "", "1", 1, 1)];
    let first = sel.ordered(&candidates);
    let second = sel.ordered(&candidates);
    // equal weights -> ring length 2; advancing cursor must rotate the start channel
    assert_ne!(first[0].id, second[0].id, "cursor should rotate which channel is tried first");
    // but both rotations still cover the full set exactly once
    let mut ids: Vec<i64> = second.iter().map(|c| c.id).collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2]);
}

// ---- model mapping: malformed mapping (injection) neutralized ----

#[test]
fn map_model_hit_and_miss() {
    let mut c = chan(1, "m", "", "1", 1, 1);
    c.model_mapping = r#"{"gpt-4o":"gpt-4o-2024"}"#.into();
    assert_eq!(map_model(&c, "gpt-4o"), "gpt-4o-2024");
    assert_eq!(map_model(&c, "unknown"), "unknown"); // miss passes through verbatim
}

#[test]
fn injection_malformed_model_mapping_falls_back_to_passthrough() {
    let mut c = chan(1, "m", "", "1", 1, 1);
    // broken JSON mapping must not panic; unknown model passes through unchanged
    c.model_mapping = "{ this is not json !!!".into();
    assert_eq!(map_model(&c, "whatever"), "whatever");
}
