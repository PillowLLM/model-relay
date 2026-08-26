//! relay-api: axum 路由 + 鉴权 + 管理接口。
mod admin;
mod auth;
mod captcha;
mod error;
mod v1;

use std::sync::Arc;

use axum::{routing::get, routing::post, routing::put, routing::delete, Router};
use relay_billing::Billing;
use relay_core::Config;
use relay_gateway::Gateway;
use relay_health::HealthService;
use relay_provider::ProviderRegistry;
use relay_store::Store;

pub use auth::{SessionStore, AdminAuth, UserAuth};
pub use captcha::CaptchaStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub gateway: Arc<Gateway>,
    pub billing: Arc<Billing>,
    pub health: Arc<HealthService>,
    pub providers: Arc<ProviderRegistry>,
    pub sessions: Arc<SessionStore>,
    pub captcha: Arc<CaptchaStore>,
    pub cfg: Config,
    pub master_key: [u8; 32],
}

pub fn router(state: AppState) -> Router {
    Router::new()
        // 转发 API（OpenAI 兼容）
        .route("/v1/chat/completions", post(v1::chat_completions))
        .route("/v1/messages", post(v1::messages))
        .route("/v1/models", get(v1::list_models))
        .route("/v1/dashboard/billing/subscription", get(v1::billing_subscription))
        .route("/v1/dashboard/billing/usage", get(v1::billing_usage))
        // 认证
        .route("/api/auth/captcha", get(captcha::get_captcha))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::me).put(auth::update_me))
        // 管理后台
        .route("/api/admin/channels", get(admin::list_channels).post(admin::create_channel))
        .route("/api/admin/channels/batch", post(admin::batch_channels))
        .route("/api/admin/channels/{id}", get(admin::get_channel).put(admin::update_channel).delete(admin::delete_channel))
        .route("/api/admin/channels/{id}/status", put(admin::update_channel_status))
        .route("/api/admin/channels/{id}/test", post(admin::test_channel))
        .route("/api/admin/tokens", get(admin::list_tokens).post(admin::create_token))
        .route("/api/admin/tokens/{id}", put(admin::update_token).delete(admin::delete_token))
        .route("/api/admin/tokens/{id}/status", put(admin::update_token_status))
        .route("/api/admin/users", get(admin::list_users).post(admin::create_user))
        .route("/api/admin/users/{id}", put(admin::update_user).delete(admin::delete_user))
        .route("/api/admin/groups", get(admin::list_groups).post(admin::create_group))
        .route("/api/admin/groups/{id}", put(admin::update_group).delete(admin::delete_group))
        .route("/api/admin/pricing", get(admin::list_pricing).post(admin::upsert_pricing))
        .route("/api/admin/pricing/{id}", delete(admin::delete_pricing))
        .route("/api/admin/redeem", get(admin::list_redeem).post(admin::create_redeem))
        .route("/api/admin/redeem/{id}/status", put(admin::update_redeem_status))
        .route("/api/admin/usage", get(admin::list_usage))
        .route("/api/admin/stats", get(admin::stats))
        .route("/api/admin/health", get(admin::health))
        // 用户自助
        .route("/api/redeem", post(v1::user_redeem))
        .route("/api/usage/my", get(v1::my_usage))
        .route("/api/recharge/plans", get(v1::recharge_plans))
        .route("/api/recharge/orders", get(v1::my_recharge_orders).post(v1::create_recharge_order))
        .with_state(state)
}
