//! 管理后台 API（全部需 AdminAuth）。
use axum::extract::{Path, Query, State};
use axum::Json;
use relay_core::{util, Channel, Group, Pricing, RedeemCode, Token, UsageFilter, User};
use relay_provider::ProviderRegistry;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::auth::AdminAuth;
use crate::error::ApiResult;
use crate::AppState;

fn now() -> String { util::now_str() }

fn mask_channel(c: &Channel, master_key: &[u8; 32]) -> Value {
    let key = relay_core::crypto::decrypt(master_key, &c.api_key_enc).unwrap_or_default();
    let masked = util::mask_key(&key);
    json!({
        "id": c.id, "name": c.name, "type": c.r#type, "base_url": c.base_url,
        "api_key_masked": masked, "models": c.models, "model_mapping": c.model_mapping,
        "priority": c.priority, "weight": c.weight, "group_ids": c.group_ids, "status": c.status,
        "auto_ban": c.auto_ban, "max_retries": c.max_retries, "timeout_ms": c.timeout_ms,
        "created_at": c.created_at,
    })
}

// ---------------- 渠道 ----------------
#[derive(Deserialize)]
pub struct ChanQuery { #[serde(default)] page: u32, #[serde(default)] page_size: u32, #[serde(default)] r#type: Option<String>, #[serde(default)] keyword: Option<String> }

pub async fn list_channels(state: State<AppState>, _a: AdminAuth, Query(q): Query<ChanQuery>) -> ApiResult<Json<Value>> {
    let size = if q.page_size == 0 { 20 } else { q.page_size };
    let mut chans = state.store.list_channels(false)?;
    if let Some(t) = &q.r#type { chans.retain(|c| &c.r#type == t); }
    if let Some(k) = &q.keyword { let k = k.to_lowercase(); chans.retain(|c| c.name.to_lowercase().contains(&k)); }
    let total = chans.len() as u32;
    let page = q.page.max(1);
    let items: Vec<Value> = chans.iter().rev().skip(((page - 1) * size) as usize).take(size as usize)
        .map(|c| mask_channel(c, &state.master_key)).collect();
    Ok(Json(json!({ "items": items, "total": total })))
}

#[derive(Deserialize)]
pub struct ChanBody {
    name: String, r#type: String, base_url: String, #[serde(default)] api_key: String,
    #[serde(default)] models: String, #[serde(default = "default_mapping")] model_mapping: String,
    #[serde(default)] priority: i32, #[serde(default = "one")] weight: i32,
    #[serde(default = "default_group_ids")] group_ids: String, #[serde(default)] status: i32,
    #[serde(default = "one")] auto_ban: i32, #[serde(default = "one")] max_retries: i32,
    #[serde(default = "default_timeout")] timeout_ms: i64, #[serde(default)] id: Option<i64>,
}
fn default_mapping() -> String { "{}".into() }
fn one() -> i32 { 1 }
fn default_group_ids() -> String { "1".into() }
fn default_timeout() -> i64 { 60000 }

fn encrypt_key(state: &AppState, plain: &str) -> String { relay_core::crypto::encrypt(&state.master_key, plain) }

pub async fn create_channel(state: State<AppState>, _a: AdminAuth, Json(b): Json<ChanBody>) -> ApiResult<Json<Value>> {
    if b.api_key.is_empty() { return Err(relay_core::AppError::bad_request("api_key 必填").into()); }
    let c = Channel {
        id: 0, name: b.name, r#type: b.r#type, base_url: b.base_url,
        api_key_enc: encrypt_key(&state, &b.api_key), models: b.models, model_mapping: b.model_mapping,
        priority: b.priority, weight: b.weight, group_ids: b.group_ids, status: b.status,
        auto_ban: b.auto_ban, max_retries: b.max_retries, timeout_ms: b.timeout_ms, created_at: now(),
    };
    let id = state.store.upsert_channel(&c)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn get_channel(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let c = state.store.get_channel(id)?.ok_or_else(|| relay_core::AppError::not_found("渠道不存在"))?;
    Ok(Json(mask_channel(&c, &state.master_key)))
}

pub async fn update_channel(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<ChanBody>) -> ApiResult<Json<Value>> {
    let existing = state.store.get_channel(id)?.ok_or_else(|| relay_core::AppError::not_found("渠道不存在"))?;
    let enc = if b.api_key.is_empty() { existing.api_key_enc } else { encrypt_key(&state, &b.api_key) };
    let c = Channel {
        id, name: b.name, r#type: b.r#type, base_url: b.base_url, api_key_enc: enc,
        models: b.models, model_mapping: b.model_mapping, priority: b.priority, weight: b.weight,
        group_ids: b.group_ids, status: b.status, auto_ban: b.auto_ban, max_retries: b.max_retries,
        timeout_ms: b.timeout_ms, created_at: existing.created_at,
    };
    state.store.upsert_channel(&c)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_channel(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    state.store.delete_channel(id)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)] pub struct StatusBody { status: i32 }
pub async fn update_channel_status(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<StatusBody>) -> ApiResult<Json<Value>> {
    state.store.update_channel_status(id, b.status)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn batch_channels(state: State<AppState>, _a: AdminAuth, Json(items): Json<Vec<ChanBody>>) -> ApiResult<Json<Value>> {
    if items.len() > 100 { return Err(relay_core::AppError::bad_request("批量导入上限 100 条").into()); }
    let mut success = 0i32; let mut failed = Vec::new();
    for (i, b) in items.into_iter().enumerate() {
        if b.api_key.is_empty() { failed.push(json!({"index": i, "error": "api_key 必填"})); continue; }
        let c = Channel {
            id: 0, name: b.name, r#type: b.r#type, base_url: b.base_url,
            api_key_enc: encrypt_key(&state, &b.api_key), models: b.models, model_mapping: b.model_mapping,
            priority: b.priority, weight: b.weight, group_ids: b.group_ids, status: b.status,
            auto_ban: b.auto_ban, max_retries: b.max_retries, timeout_ms: b.timeout_ms, created_at: now(),
        };
        match state.store.upsert_channel(&c) { Ok(_) => success += 1, Err(e) => failed.push(json!({"index": i, "error": e.message()})) }
    }
    Ok(Json(json!({ "success": success, "failed": failed })))
}

pub async fn test_channel(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let c = state.store.get_channel(id)?.ok_or_else(|| relay_core::AppError::not_found("渠道不存在"))?;
    let p = state.providers.get(&c.r#type)?;
    let t0 = std::time::Instant::now();
    let models = p.list_models(&c).await?;
    Ok(Json(json!({ "ok": true, "latency_ms": t0.elapsed().as_millis(), "models_count": models.len() })))
}

// ---------------- 令牌 ----------------
#[derive(Deserialize)] pub struct TokenQuery { #[serde(default)] user_id: Option<i64>, #[serde(default)] page: u32, #[serde(default)] page_size: u32 }
#[derive(Deserialize)] pub struct TokenBody { user_id: i64, name: String, #[serde(default = "neg_one")] quota_limit: i64, #[serde(default)] expired_at: Option<String> }
fn neg_one() -> i64 { -1 }

pub async fn list_tokens(state: State<AppState>, _a: AdminAuth, Query(q): Query<TokenQuery>) -> ApiResult<Json<Value>> {
    let size = if q.page_size == 0 { 20 } else { q.page_size };
    let (items, total) = state.store.list_tokens(q.user_id, q.page.max(1), size)?;
    Ok(Json(json!({ "items": items, "total": total })))
}

pub async fn create_token(state: State<AppState>, _a: AdminAuth, Json(b): Json<TokenBody>) -> ApiResult<Json<Value>> {
    let plain = util::gen_token();
    let hash = util::sha256_hex(&plain);
    let t = Token {
        id: 0, user_id: b.user_id, name: b.name, key_hash: hash, status: 1,
        quota_limit: b.quota_limit, used_quota: 0, expired_at: b.expired_at, created_at: now(),
    };
    let id = state.store.create_token(&t)?;
    Ok(Json(json!({ "id": id, "token": plain, "name": t.name, "quota_limit": t.quota_limit, "expired_at": t.expired_at })))
}

#[derive(Deserialize)] pub struct TokenUpdate { name: Option<String>, #[serde(default)] quota_limit: Option<i64>, #[serde(default)] expired_at: Option<Option<String>> }
pub async fn update_token(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<TokenUpdate>) -> ApiResult<Json<Value>> {
    let mut t = state.store.list_tokens(None, 1, 10000)?.0.into_iter().find(|t| t.id == id).ok_or_else(|| relay_core::AppError::not_found("令牌不存在"))?;
    if let Some(n) = b.name { t.name = n; }
    if let Some(q) = b.quota_limit { t.quota_limit = q; }
    if let Some(e) = b.expired_at { t.expired_at = e; }
    state.store.update_token(&t)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_token(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    state.store.delete_token(id)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn update_token_status(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<StatusBody>) -> ApiResult<Json<Value>> {
    state.store.update_token_status(id, b.status)?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------- 用户 / 分组 ----------------
#[derive(Deserialize)] pub struct PageQuery { #[serde(default)] page: u32, #[serde(default)] page_size: u32 }
#[derive(Deserialize)] pub struct UserBody {
    username: Option<String>, #[serde(default)] password: Option<String>, #[serde(default)] role: Option<String>,
    #[serde(default)] group_id: Option<i64>, #[serde(default)] quota_limit: Option<i64>,
    #[serde(default)] email: Option<String>, #[serde(default)] phone: Option<String>, #[serde(default)] display_name: Option<String>,
    #[serde(default)] status: Option<i32>,
}

pub async fn list_users(state: State<AppState>, _a: AdminAuth, Query(q): Query<PageQuery>) -> ApiResult<Json<Value>> {
    let size = if q.page_size == 0 { 20 } else { q.page_size };
    let (mut items, total) = state.store.list_users(q.page.max(1), size)?;
    for u in items.iter_mut() { u.password_hash = String::new(); }
    Ok(Json(json!({ "items": items, "total": total })))
}

pub async fn create_user(state: State<AppState>, _a: AdminAuth, Json(b): Json<UserBody>) -> ApiResult<Json<Value>> {
    let username = b.username.ok_or_else(|| relay_core::AppError::bad_request("username 必填"))?;
    let password = b.password.ok_or_else(|| relay_core::AppError::bad_request("password 必填"))?;
    if password.len() < 8 { return Err(relay_core::AppError::bad_request("password 至少 8 位").into()); }
    let u = User {
        id: 0, username, password_hash: relay_core::crypto::hash_password(&password),
        display_name: b.display_name.unwrap_or_default(), email: b.email.unwrap_or_default(),
        phone: b.phone.unwrap_or_default(), role: b.role.unwrap_or_else(|| "user".into()),
        group_id: b.group_id.unwrap_or(1), quota_limit: b.quota_limit.unwrap_or(-1),
        used_quota: 0, status: b.status.unwrap_or(1), created_at: now(),
    };
    let id = state.store.create_user(&u)?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update_user(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<UserBody>) -> ApiResult<Json<Value>> {
    let mut u = state.store.get_user_by_id(id)?.ok_or_else(|| relay_core::AppError::not_found("用户不存在"))?;
    if let Some(d) = b.display_name { u.display_name = d; }
    if let Some(e) = b.email { u.email = e; }
    if let Some(p) = b.phone { u.phone = p; }
    if let Some(r) = b.role { u.role = r; }
    if let Some(g) = b.group_id { u.group_id = g; }
    if let Some(q) = b.quota_limit { u.quota_limit = q; }
    if let Some(s) = b.status { u.status = s; }
    u.password_hash = match b.password { Some(p) if !p.is_empty() => relay_core::crypto::hash_password(&p), _ => String::new() };
    state.store.update_user(&u)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_user(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    state.store.delete_user(id)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn list_groups(state: State<AppState>, _a: AdminAuth) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": state.store.list_groups()?, "total": 0 })))
}

#[derive(Deserialize)] pub struct GroupBody { name: String, #[serde(default = "one_f")] quota_multiplier: f64, #[serde(default)] remark: String }
fn one_f() -> f64 { 1.0 }
pub async fn create_group(state: State<AppState>, _a: AdminAuth, Json(b): Json<GroupBody>) -> ApiResult<Json<Value>> {
    let g = Group { id: 0, name: b.name, quota_multiplier: b.quota_multiplier, remark: b.remark, created_at: now() };
    let id = state.store.upsert_group(&g)?;
    Ok(Json(json!({ "id": id })))
}
pub async fn update_group(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<GroupBody>) -> ApiResult<Json<Value>> {
    let g = Group { id, name: b.name, quota_multiplier: b.quota_multiplier, remark: b.remark, created_at: String::new() };
    state.store.upsert_group(&g)?;
    Ok(Json(json!({ "ok": true })))
}
pub async fn delete_group(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    state.store.delete_group(id)?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------- 定价 ----------------
pub async fn list_pricing(state: State<AppState>, _a: AdminAuth) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": state.store.list_pricing()? })))
}
#[derive(Deserialize)] pub struct PricingBody { model: String, #[serde(default)] prompt_price: f64, #[serde(default)] completion_price: f64 }
pub async fn upsert_pricing(state: State<AppState>, _a: AdminAuth, Json(b): Json<PricingBody>) -> ApiResult<Json<Value>> {
    state.store.upsert_pricing(&Pricing { id: 0, model: b.model, prompt_price: b.prompt_price, completion_price: b.completion_price, created_at: now() })?;
    Ok(Json(json!({ "ok": true })))
}
pub async fn delete_pricing(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    state.store.delete_pricing(id)?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------- 兑换码 ----------------
#[derive(Deserialize)] pub struct RedeemQuery { #[serde(default)] status: Option<i32>, #[serde(default)] page: u32, #[serde(default)] page_size: u32 }
pub async fn list_redeem(state: State<AppState>, _a: AdminAuth, Query(q): Query<RedeemQuery>) -> ApiResult<Json<Value>> {
    let size = if q.page_size == 0 { 20 } else { q.page_size };
    let (items, total) = state.store.list_redeem(q.status, q.page.max(1), size)?;
    Ok(Json(json!({ "items": items, "total": total })))
}
#[derive(Deserialize)] pub struct RedeemBody { #[serde(default = "one_u")] count: u32, quota: i64, #[serde(default)] expires_at: Option<String> }
fn one_u() -> u32 { 1 }
pub async fn create_redeem(state: State<AppState>, _a: AdminAuth, Json(b): Json<RedeemBody>) -> ApiResult<Json<Value>> {
    if b.count == 0 || b.count > 100 { return Err(relay_core::AppError::bad_request("count 需 1-100").into()); }
    let mut codes = Vec::new();
    for _ in 0..b.count {
        let code = util::gen_redeem_code();
        let r = RedeemCode { id: 0, code: code.clone(), quota: b.quota, status: 0, used_by: None, used_at: None, expires_at: b.expires_at.clone(), remark: String::new(), created_at: now() };
        state.store.create_redeem(&r)?;
        codes.push(code);
    }
    Ok(Json(json!({ "codes": codes })))
}
pub async fn update_redeem_status(state: State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<StatusBody>) -> ApiResult<Json<Value>> {
    state.store.update_redeem_status(id, b.status)?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------- 用量 / 统计 / 健康 ----------------
#[derive(Deserialize)] pub struct UsageQuery {
    #[serde(default)] user_id: Option<i64>, #[serde(default)] channel_id: Option<i64>,
    #[serde(default)] model: Option<String>, #[serde(default)] start: Option<String>,
    #[serde(default)] end: Option<String>, #[serde(default)] page: u32, #[serde(default)] page_size: u32,
}
pub async fn list_usage(state: State<AppState>, _a: AdminAuth, Query(q): Query<UsageQuery>) -> ApiResult<Json<Value>> {
    let (items, total) = state.store.query_usage(&UsageFilter {
        user_id: q.user_id, channel_id: q.channel_id, model: q.model, start: q.start, end: q.end,
        page: q.page.max(1), page_size: if q.page_size == 0 { 20 } else { q.page_size },
    })?;
    Ok(Json(json!({ "items": items, "total": total })))
}

#[derive(Deserialize)] pub struct StatsQuery { #[serde(default = "thirty")] days: u32 }
fn thirty() -> u32 { 30 }
pub async fn stats(state: State<AppState>, _a: AdminAuth, Query(q): Query<StatsQuery>) -> ApiResult<Json<Value>> {
    Ok(Json(serde_json::to_value(state.store.usage_stats(q.days)?).unwrap_or_default()))
}

pub async fn health(state: State<AppState>, _a: AdminAuth) -> ApiResult<Json<Value>> {
    Ok(Json(serde_json::to_value(state.health.report()).unwrap_or_default()))
}

#[allow(dead_code)]
fn _unused(_p: &ProviderRegistry, _m: &HashMap<String, String>) {}
