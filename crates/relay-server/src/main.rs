//! model-relay 服务入口。
use std::sync::Arc;

use relay_api::{AppState, CaptchaStore, SessionStore};
use relay_billing::Billing;
use relay_core::{crypto, util, Config, User};
use relay_gateway::Gateway;
use relay_health::{FailureTracker, HealthService};
use relay_provider::ProviderRegistry;
use relay_store::{SqliteStore, Store};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cfg = Config::from_env();
    print_banner(&cfg);

    // 自检
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open(&cfg.db_path())?);
    store.init()?;
    HealthService::startup_checks(&cfg, store.as_ref()).map_err(|e| format!("启动自检失败: {e}"))?;

    // 初始管理员
    ensure_admin(&store, &cfg)?;

    // 组装服务
    let providers = Arc::new(ProviderRegistry::new(cfg.master_key));
    let billing = Arc::new(Billing::new(store.clone()));
    let failures = Arc::new(FailureTracker::new());
    let gateway = Arc::new(Gateway::new(store.clone(), billing.clone(), providers.clone(), failures.clone(), cfg.clone()));
    let health = Arc::new(HealthService::new(store.clone(), providers.clone(), cfg.clone(), failures.clone()));

    relay_health::spawn_health_loop(health.clone());

    let state = AppState {
        store: store.clone(),
        gateway,
        billing,
        health,
        providers,
        sessions: Arc::new(SessionStore::new()),
        captcha: Arc::new(CaptchaStore::new()),
        cfg: cfg.clone(),
        master_key: cfg.master_key,
    };

    let app = relay_api::router(state)
        .fallback_service(ServeDir::new(&cfg.public_dir))
        .layer(TraceLayer::new_for_http());

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Relay 已启动: http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

fn ensure_admin(store: &Arc<dyn Store>, cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if store.get_user_by_username(&cfg.admin_username)?.is_none() {
        let u = User {
            id: 0,
            username: cfg.admin_username.clone(),
            password_hash: crypto::hash_password(&cfg.admin_password),
            display_name: "Administrator".into(),
            email: String::new(),
            phone: String::new(),
            role: "admin".into(),
            group_id: 1,
            quota_limit: -1,
            used_quota: 0,
            status: 1,
            created_at: util::now_str(),
        };
        store.create_user(&u)?;
        tracing::info!("已创建初始管理员: {}", cfg.admin_username);
    }
    Ok(())
}

fn print_banner(cfg: &Config) {
    let b = r#"
  __  __       _    __           _    ____           _        _ _
 | \/ (_)_ __ (_) / _|_ _ __ _ / \  |  _ \ ___ _ __| |_ __ _| | | __ _
 | |\/| | '_ \| | |_| '__/ _` / _ \ | |_) / _ \ '__| __/ _` | | |/ _` |
 | |  | | |_) | |  _| | | (_|/ ___ \|  _ <  __/ |  | || (_| | | | (_| |
 |_|  |_| .__/|_|_| |_|  \__,/_/   \_\_| \_\___|_|   \__\__,_|_|_|\__,_|
        |_|
"#;
    println!("{b}");
    println!("  模型中转站 · model-relay v0.1 (Rust)");
    println!("  端口: {}  数据目录: {}  公共目录: {}", cfg.port, cfg.data_dir, cfg.public_dir);
    if cfg.disable_captcha {
        println!("  ⚠ 验证码已禁用（RELAY_DISABLE_CAPTCHA=1）");
    }
    println!("  启动时间: {}", util::now_str());
    println!();
}
