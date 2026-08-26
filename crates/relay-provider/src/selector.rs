//! 渠道选择：priority DESC → 同优先级内按 weight 轮询。
use std::sync::atomic::{AtomicUsize, Ordering};

use relay_core::Channel;

pub struct ChannelSelector {
    cursor: AtomicUsize,
}

impl ChannelSelector {
    pub fn new() -> Self {
        Self { cursor: AtomicUsize::new(0) }
    }

    /// 输入已按 priority DESC 排序的候选，按 weight 轮询选出尝试顺序（全部，供重试/故障转移逐个尝试）。
    pub fn ordered<'a>(&self, candidates: &'a [Channel]) -> Vec<&'a Channel> {
        if candidates.is_empty() {
            return vec![];
        }
        // 展开加权环
        let mut ring: Vec<&Channel> = Vec::new();
        for c in candidates {
            for _ in 0..c.weight.max(1) {
                ring.push(c);
            }
        }
        if ring.is_empty() {
            return vec![];
        }
        let start = self.cursor.fetch_add(1, Ordering::Relaxed) % ring.len();
        // 从 start 开始展开完整一轮（去重保序）
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for i in 0..ring.len() {
            let c = ring[(start + i) % ring.len()];
            if seen.insert(c.id) {
                out.push(c);
            }
        }
        out
    }
}

impl Default for ChannelSelector {
    fn default() -> Self {
        Self::new()
    }
}
