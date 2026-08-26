//! 流式转换：canonical StreamEvent → 客户端 SSE 行。
use relay_core::StreamEvent;
use serde_json::{json, Value};

use crate::detect::InboundFormat;

/// 把一个 canonical 事件转成客户端 SSE 行（含 `data: ` 前缀，返回 None 表示跳过）。
/// Anthropic 客户端额外带 `event: ` 行（用 `event:...\n` 前缀表示）。
pub fn event_to_sse(ev: &StreamEvent, fmt: InboundFormat, id: &str, model: &str) -> Option<String> {
    match fmt {
        InboundFormat::Anthropic => anthropic_sse(ev, model),
        _ => openai_sse(ev, id, model),
    }
}

fn openai_sse(ev: &StreamEvent, id: &str, model: &str) -> Option<String> {
    match ev {
        StreamEvent::TextDelta { text } => {
            let chunk = json!({
                "id": id, "object": "chat.completion.chunk", "model": model,
                "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": Value::Null }]
            });
            Some(format!("data: {}\n\n", chunk))
        }
        StreamEvent::Usage { usage } => {
            let chunk = json!({
                "id": id, "object": "chat.completion.chunk", "model": model,
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": usage.prompt_tokens, "completion_tokens": usage.completion_tokens, "total_tokens": usage.total_tokens }
            });
            Some(format!("data: {}\n\n", chunk))
        }
        StreamEvent::Done => Some("data: [DONE]\n\n".into()),
        _ => None,
    }
}

fn anthropic_sse(ev: &StreamEvent, model: &str) -> Option<String> {
    match ev {
        StreamEvent::TextDelta { text } => {
            let data = json!({
                "type": "content_block_delta", "index": 0,
                "delta": { "type": "text_delta", "text": text }
            });
            Some(format!("event: content_block_delta\ndata: {}\n\n", data))
        }
        StreamEvent::Usage { usage } => {
            let data = json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn" },
                "usage": { "output_tokens": usage.completion_tokens }
            });
            Some(format!("event: message_delta\ndata: {}\n\n", data))
        }
        StreamEvent::Done => Some("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into()),
        _ => None,
    }
}
