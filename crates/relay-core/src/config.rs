use rand::RngCore;

/// 全局配置（环境变量前缀 RELAY_）。
#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub data_dir: String,
    pub public_dir: String,
    pub admin_username: String,
    pub admin_password: String,
    pub master_key: [u8; 32],
    pub disable_captcha: bool,
    pub default_max_tokens: u32,
    pub default_timeout_ms: u64,
    pub max_retries: u32,
    pub health_interval_secs: u64,
    pub rate_limit_per_min: u32,
    pub tokenizer_family_default: String,
}

impl Config {
    pub fn from_env() -> Self {
        let master_key = load_or_gen_master_key();
        Self {
            port: env_u16("RELAY_PORT", 8322),
            data_dir: env_str("RELAY_DATA_DIR", "./data"),
            public_dir: env_str("RELAY_PUBLIC_DIR", "./public"),
            admin_username: env_str("RELAY_ADMIN_USERNAME", "admin"),
            admin_password: std::env::var("RELAY_ADMIN_PASSWORD").unwrap_or_default(),
            master_key,
            disable_captcha: std::env::var("RELAY_DISABLE_CAPTCHA").as_deref() == Ok("1"),
            default_max_tokens: 4096,
            default_timeout_ms: 60000,
            max_retries: 1,
            health_interval_secs: env_u64("RELAY_HEALTH_INTERVAL", 300),
            rate_limit_per_min: env_u32("RELAY_RATE_LIMIT", 60),
            tokenizer_family_default: "cl100k_base".into(),
        }
    }

    /// 启动自检用：admin_password 非空、master_key 就绪。
    pub fn validate(&self) -> Result<(), String> {
        if self.admin_password.is_empty() {
            return Err("RELAY_ADMIN_PASSWORD 未设置，拒绝启动".into());
        }
        if self.master_key.len() != 32 {
            return Err("RELAY_MASTER_KEY 必须为 32 字节".into());
        }
        Ok(())
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        let p = std::path::Path::new(&self.data_dir);
        std::fs::create_dir_all(p).ok();
        p.join("relay.db")
    }
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 加载 RELAY_MASTER_KEY（32 字节，hex 编码 64 字符）；缺失则生成并打印一次。
fn load_or_gen_master_key() -> [u8; 32] {
    if let Ok(hex) = std::env::var("RELAY_MASTER_KEY") {
        if let Some(k) = hex_to_32(&hex) {
            return k;
        }
        tracing::warn!("RELAY_MASTER_KEY 非法（需 64 位 hex），将生成临时密钥");
    }
    let mut k = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut k);
    let hex = bytes_to_hex(&k);
    tracing::warn!(
        "未提供 RELAY_MASTER_KEY，已生成临时密钥（请保存为环境变量 RELAY_MASTER_KEY，否则重启后渠道 Key 无法解密）: {}",
        hex
    );
    k
}

fn hex_to_32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
