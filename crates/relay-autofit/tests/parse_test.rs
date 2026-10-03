//! relay-autofit 入站识别 + 解析测试，重点覆盖畸形/恶意载荷（注入测试）。
use http::HeaderMap;
use relay_autofit::{detect_inbound, parse, InboundFormat};
use serde_json::json;

#[test]
fn detect_by_explicit_path() {
    let h = HeaderMap::new();
    assert_eq!(detect_inbound("/v1/chat/completions", &h, &json!({})), InboundFormat::OpenAi);
    assert_eq!(detect_inbound("/v1/messages", &h, &json!({})), InboundFormat::Anthropic);
    assert_eq!(
        detect_inbound("/v1beta/models/gemini:generateContent", &h, &json!({})),
        InboundFormat::Gemini
    );
}

#[test]
fn detect_sniffs_body_when_path_unknown() {
    let h = HeaderMap::new();
    assert_eq!(detect_inbound("/x", &h, &json!({"messages": []})), InboundFormat::OpenAi);
    assert_eq!(detect_inbound("/x", &h, &json!({"contents": []})), InboundFormat::Gemini);
    assert_eq!(detect_inbound("/x", &h, &json!({})), InboundFormat::Unknown);
}

#[test]
fn parse_openai_happy_path() {
    let body = json!({
        "model": "gpt-4o",
        "messages": [{"role":"user","content":"hi"}],
        "stream": false
    });
    let req = parse(InboundFormat::OpenAi, &body).expect("valid openai body");
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 1);
}

// ---- INJECTION / malformed payload: must be rejected or neutralized, never panic ----

#[test]
fn injection_openai_missing_or_oversized_messages_rejected() {
    // missing model
    assert!(parse(InboundFormat::OpenAi, &json!({"messages":[{"role":"user","content":"x"}]})).is_err());
    // missing messages
    assert!(parse(InboundFormat::OpenAi, &json!({"model":"m"})).is_err());
    // empty messages
    assert!(parse(InboundFormat::OpenAi, &json!({"model":"m","messages":[]})).is_err());
    // >512 messages
    let big = json!({"model":"m","messages": vec![json!({"role":"user","content":"x"}); 600]});
    assert!(parse(InboundFormat::OpenAi, &big).is_err());
}

#[test]
fn injection_model_too_long_rejected() {
    let long: String = "m".repeat(65);
    let body = json!({"model": long, "messages":[{"role":"user","content":"x"}]});
    assert!(parse(InboundFormat::OpenAi, &body).is_err());
}

#[test]
fn injection_xss_in_model_and_content_is_passed_through_verbatim_without_panic() {
    // Upstream relay should NOT try to interpret/escape; it must treat the string as opaque data.
    let evil = "<script>alert('xss')</script>'; DROP TABLE users;--";
    let body = json!({
        "model": evil,
        "messages":[{"role":"user","content": evil}]
    });
    let req = parse(InboundFormat::OpenAi, &body).expect("opaque strings are accepted as data");
    assert_eq!(req.model, evil);
}

#[test]
fn injection_malformed_tool_arguments_do_not_panic() {
    // arguments is broken JSON -> parser must fall back to Null instead of crashing
    let body = json!({
        "model":"m",
        "messages":[{
            "role":"assistant",
            "content":"",
            "tool_calls":[{"id":"c1","function":{"name":"get_weather","arguments":"{not valid json!!"}}]
        }]
    });
    let req = parse(InboundFormat::OpenAi, &body).expect("malformed args tolerated");
    assert_eq!(req.messages.len(), 1);
}

#[test]
fn injection_content_of_wrong_type_rejected() {
    // content as a number (not string/array)
    let body = json!({"model":"m","messages":[{"role":"user","content":12345}]});
    assert!(parse(InboundFormat::OpenAi, &body).is_err());
}

#[test]
fn anthropic_requires_max_tokens() {
    let body = json!({"model":"claude","messages":[{"role":"user","content":"hi"}]});
    assert!(parse(InboundFormat::Anthropic, &body).is_err());
    let ok = json!({"model":"claude","max_tokens":128,"messages":[{"role":"user","content":"hi"}]});
    assert!(parse(InboundFormat::Anthropic, &ok).is_ok());
}

#[test]
fn unknown_format_rejected() {
    assert!(parse(InboundFormat::Unknown, &json!({})).is_err());
    assert!(parse(InboundFormat::Gemini, &json!({})).is_err());
}
