//! 限流器：滑动窗口（每分钟）。
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimiter {
    map: Mutex<std::collections::HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self { map: Mutex::new(std::collections::HashMap::new()) }
    }

    /// 检查 key 在 60s 内是否超过 limit；通过则记录当前时间。
    pub fn check(&self, key: &str, limit_per_min: u32) -> bool {
        let mut m = self.map.lock().unwrap();
        let q = m.entry(key.to_string()).or_default();
        let now = Instant::now();
        while let Some(front) = q.front() {
            if now.duration_since(*front).as_secs() >= 60 {
                q.pop_front();
            } else {
                break;
            }
        }
        if (q.len() as u32) >= limit_per_min {
            return false;
        }
        q.push_back(now);
        true
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
