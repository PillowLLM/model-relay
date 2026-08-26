//! 转发 API + 用户自助。
use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use relay_core::{util, AppError, Page, UsageFilter};
use serde_json::{json, Value};

use crate::auth::UserAuth;
use crate::error::ApiResult;
use crate::AppState;

fn extract_token(headers: &HeaderMap, query: &HashMap<String, String>) -> Option<String> {
    if let Some(auth) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if let Some(b) = auth.strip_prefix("Bearer ") {
            return Some(b.trim().to_string());
        }
    }
    if let Some(k) = headers.get("x-api-key").and_then(|h| h.to_str().ok()) {
        return Some(k.to_string());
    }
    query.get("token").cloned()
}

async fn forward_chat(
    state: &AppState,
    path: &str,
    headers: HeaderMap,
    query: HashMap<String, String>,
    raw: Value,
) -> ApiResult<Response> {
    let token = extract_token(&headers, &query);
    match state.gateway.handle_chat(path, &headers, token, raw).await? {
        relay_gateway::ChatOutcome::NonStream(v) => Ok(Json(v).into_response()),
        relay_gateway::ChatOutcome::Stream(sse) => {
            let body = Body::from_stream(sse.map(|r| {
                let bytes = match r {
                    Ok(s) => axum::body::Bytes::from(s),
                    Err(e) => axum::body::Bytes::from(format!(
                        "data: {{\"error\":{{\"message\":\"{}\",\"code\":{}}}}}\n\n",
                        e.message(), e.status()
                    )),
                };
                Ok::<_, std::convert::Infallible>(bytes)
            }));
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream; charset=utf-8")
                .header("cache-control", "no-cache")
                .header("x-accel-buffering", "no")
                .body(body)
                .unwrap())
        }
    }
}

pub async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(raw): Json<Value>,
) -> ApiResult<Response> {
    forward_chat(&state, "/v1/chat/completions", headers, query, raw).await
}

pub async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(raw): Json<Value>,
) -> ApiResult<Response> {
    forward_chat(&state, "/v1/messages", headers, query, raw).await
}

pub async fn list_models(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> ApiResult<Json<Value>> {
    let _ = extract_token(&headers, &query); // 鉴权在 gateway；此处仅占位
    Ok(Json(state.gateway.list_models()?))
}

pub async fn billing_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let token_str = extract_token(&headers, &query).ok_or_else(|| AppError::unauthorized("缺少令牌"))?;
    let (token, _, _) = state.gateway.authenticate(Some(token_str))?;
    Ok(Json(state.gateway.billing_subscription(&token)))
}

pub async fn billing_usage(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> ApiResult<Json<Value>> {
    let token_str = extract_token(&headers, &query).ok_or_else(|| AppError::unauthorized("缺少令牌"))?;
    let (token, user, _) = state.gateway.authenticate(Some(token_str))?;
    let (logs, _total) = state.store.query_usage(&UsageFilter {
        user_id: Some(user.id), page: 1, page_size: 1000, ..Default::default()
    })?;
    let total: i64 = logs.iter().map(|l| (l.prompt_tokens as i64) + (l.completion_tokens as i64)).sum();
    Ok(Json(json!({ "object": "list", "total_usage": total, "daily_costs": [] })))
}

// ---- 用户自助 ----

pub async fn user_redeem(State(state): State<AppState>, UserAuth(user): UserAuth, Json(req): Json<Value>) -> ApiResult<Json<Value>> {
    let code = req.get("code").and_then(Value::as_str).ok_or_else(|| AppError::bad_request("code 缺失"))?;
    let quota = state.billing.redeem_code(code, user.id)?;
    Ok(Json(json!({ "ok": true, "quota": quota })))
}

pub async fn my_usage(State(state): State<AppState>, UserAuth(user): UserAuth, Query(q): Query<HashMap<String, String>>) -> ApiResult<Json<Page<relay_core::UsageLog>>> {
    let mut filter = UsageFilter { user_id: Some(user.id), page: 1, page_size: 50, ..Default::default() };
    if let Some(s) = q.get("start") { filter.start = Some(s.clone()); }
    if let Some(e) = q.get("end") { filter.end = Some(e.clone()); }
    let (items, total) = state.store.query_usage(&filter)?;
    Ok(Json(Page { items, total }))
}

pub async fn recharge_plans(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let plans = state.store.get_setting("recharge_plans")?.unwrap_or_else(|| "[]".into());
    let plans: Value = serde_json::from_str(&plans).unwrap_or(json!([]));
    Ok(Json(json!({ "plans": plans })))
}

pub async fn create_recharge_order(State(state): State<AppState>, UserAuth(user): UserAuth, Json(req): Json<Value>) -> ApiResult<Json<Value>> {
    // M1 stub：manual 收款码
    let plan_id = req.get("plan_id").and_then(Value::as_str).unwrap_or("");
    let plans = state.store.get_setting("recharge_plans")?.unwrap_or_else(|| "[]".into());
    let plans: Vec<Value> = serde_json::from_str(&plans).unwrap_or_default();
    let plan = plans.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(plan_id))
        .ok_or_else(|| AppError::bad_request("档位不存在"))?;
    let order_no = util::gen_order_no();
    let qr = state.store.get_setting("qr_manual_url")?.unwrap_or_default();
    Ok(Json(json!({ "order_no": order_no, "qr_url": qr, "amount": plan.get("amount"), "quota": plan.get("quota") })))
}

pub async fn my_recharge_orders(_state: State<AppState>, UserAuth(_user): UserAuth) -> ApiResult<Json<Value>> {
    // M1：未实现订单查询，返回空
    Ok(Json(json!({ "items": [], "total": 0 })))
}
