//! relay-health: 启动自检 + 失败跟踪 + 渠道探测。
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use relay_core::{AppError, Channel, Config};
use relay_provider::ProviderRegistry;
use relay_store::Store;
use serde::Serialize;

const COOLDOWN_SECS: u64 = 60;
const AUTO_BAN_THRESHOLD: u32 = 5;

/// 失败跟踪器（gateway 写入，health 读取/禁用）。
pub struct FailureTracker {
    map: Mutex<HashMap<i64, (u32, Instant)>>,
}

impl FailureTracker {
    pub fn new() -> Self {
        Self { map: Mutex::new(HashMap::new()) }
    }
    pub fn note_failure(&self, channel_id: i64) {
        let mut m = self.map.lock().unwrap();
        let e = m.entry(channel_id).or_insert((0, Instant::now()));
        e.0 += 1;
        e.1 = Instant::now();
    }
    pub fn reset(&self, channel_id: i64) {
        self.map.lock().unwrap().remove(&channel_id);
    }
    pub fn consecutive(&self, channel_id: i64) -> u32 {
        self.map.lock().unwrap().get(&channel_id).map(|(n, _)| *n).unwrap_or(0)
    }
    /// 最近 60s 内失败过 → 跳过选择。
    pub fn is_cooldown(&self, channel_id: i64) -> bool {
        let m = self.map.lock().unwrap();
        m.get(&channel_id).map(|(_, t)| t.elapsed() < Duration::from_secs(COOLDOWN_SECS)).unwrap_or(false)
    }
}

impl Default for FailureTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: String, // ok / warn / fail
    pub detail: String,
    pub latency_ms: u64,
}

#[derive(Clone, Serialize)]
pub struct ChannelHealth {
    pub channel_id: i64,
    pub name: String,
    pub healthy: bool,
    pub latency_ms: u64,
    pub consecutive_failures: u32,
    pub last_error: String,
}

#[derive(Clone, Serialize, Default)]
pub struct HealthReport {
    pub ok: bool,
    pub started_at: String,
    pub checks: Vec<CheckResult>,
    pub channels: Vec<ChannelHealth>,
}

pub struct HealthService {
    pub store: Arc<dyn Store>,
    pub providers: Arc<ProviderRegistry>,
    pub cfg: Config,
    pub failures: Arc<FailureTracker>,
    pub last_report: RwLock<HealthReport>,
}

impl HealthService {
    pub fn new(store: Arc<dyn Store>, providers: Arc<ProviderRegistry>, cfg: Config, failures: Arc<FailureTracker>) -> Self {
        Self { store, providers, cfg, failures, last_report: RwLock::new(HealthReport::default()) }
    }

    /// 启动自检（fail-fast）。
    pub fn startup_checks(cfg: &Config, store: &dyn Store) -> Result<(), String> {
        let mut checks = Vec::new();
        // 1. 配置
        match cfg.validate() {
            Ok(()) => checks.push(("config", "ok", "配置校验通过")),
            Err(e) => return Err(format!("配置自检失败: {e}")),
        }
        // 2. 数据库完整性
        match store.integrity_check() {
            Ok(s) if s == "ok" => checks.push(("db_integrity", "ok", "数据库完整性 OK")),
            Ok(s) => return Err(format!("数据库完整性异常: {s}")),
            Err(e) => return Err(format!("数据库自检失败: {e}")),
        }
        // 3. 默认分组
        match store.get_group(1) {
            Ok(Some(_)) => checks.push(("default_group", "ok", "默认分组存在")),
            _ => return Err("默认分组（id=1）缺失".into()),
        }
        tracing::info!("启动自检通过: {} 项", checks.len());
        Ok(())
    }

    /// 周期探测所有启用渠道。
    pub async fn run_probe(&self) -> HealthReport {
        let channels = self.store.list_channels(true).unwrap_or_default();
        let mut ch_health = Vec::new();
        for c in &channels {
            let (healthy, latency_ms, last_error) = match self.providers.get(&c.r#type) {
                Ok(p) => match tokio::time::timeout(Duration::from_secs(3), p.list_models(c)).await {
                    Ok(Ok(models)) => {
                        self.failures.reset(c.id);
                        (true, 0, format!("{} models", models.len()))
                    }
                    Ok(Err(e)) => {
                        self.failures.note_failure(c.id);
                        let fails = self.failures.consecutive(c.id);
                        if fails >= AUTO_BAN_THRESHOLD && c.auto_ban == 1 {
                            let _ = self.store.update_channel_status(c.id, 0);
                            tracing::warn!(channel_id = c.id, "渠道连续失败 {} 次，自动禁用", fails);
                        }
                        (false, 0, e.message().to_string())
                    }
                    Err(_) => {
                        self.failures.note_failure(c.id);
                        (false, 0, "探测超时".into())
                    }
                },
                Err(e) => (false, 0, e.message().to_string()),
            };
            ch_health.push(ChannelHealth {
                channel_id: c.id, name: c.name.clone(), healthy, latency_ms,
                consecutive_failures: self.failures.consecutive(c.id), last_error,
            });
        }
        let ok = ch_health.iter().all(|c| c.healthy);
        HealthReport { ok, started_at: relay_core::util::now_str(), checks: vec![], channels: ch_health }
    }

    pub fn report(&self) -> HealthReport {
        self.last_report.read().unwrap().clone()
    }
}

/// 启动周期探测任务。
pub fn spawn_health_loop(svc: Arc<HealthService>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(svc.cfg.health_interval_secs));
        loop {
            tick.tick().await;
            let report = svc.run_probe().await;
            *svc.last_report.write().unwrap() = report;
        }
    });
}

/// 探测后自检不阻塞启动的占位。
#[allow(dead_code)]
fn _unused(_e: AppError, _c: &Channel) {}
