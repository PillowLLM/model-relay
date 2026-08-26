//! 管理 Session：内存 HashMap + Cookie。
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::Rng;
use relay_core::{crypto, AppError, User};
use serde::Deserialize;
use serde_json::json;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

const SESSION_TTL: Duration = Duration::from_secs(604800); // 7 天

pub struct SessionStore {
    map: Mutex<std::collections::HashMap<String, (i64, Instant)>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self { map: Mutex::new(std::collections::HashMap::new()) }
    }
    pub fn create(&self, user_id: i64) -> String {
        let id = format!("{:032x}", rand::thread_rng().gen::<u128>());
        self.map.lock().unwrap().insert(id.clone(), (user_id, Instant::now() + SESSION_TTL));
        id
    }
    pub fn get(&self, sid: &str) -> Option<i64> {
        let m = self.map.lock().unwrap();
        m.get(sid).filter(|(_, exp)| *exp > Instant::now()).map(|(uid, _)| *uid)
    }
    pub fn remove(&self, sid: &str) {
        self.map.lock().unwrap().remove(sid);
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

fn session_id(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in cookie.split(';') {
        let pair = pair.trim();
        if let Some(v) = pair.strip_prefix("relay_session=") {
            return Some(v.to_string());
        }
    }
    None
}

/// 要求登录用户。
pub struct UserAuth(pub User);

impl FromRequestParts<AppState> for UserAuth {
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let uid = session_id(&parts.headers)
            .and_then(|sid| state.sessions.get(&sid))
            .ok_or_else(|| ApiError(AppError::unauthorized("未登录")).into_response())?;
        let user = state.store.get_user_by_id(uid).map_err(|e| ApiError(e).into_response())?
            .ok_or_else(|| ApiError(AppError::unauthorized("用户不存在")).into_response())?;
        if user.status == 0 {
            return Err(ApiError(AppError::forbidden("账号已禁用")).into_response());
        }
        Ok(UserAuth(user))
    }
}

/// 要求管理员。
pub struct AdminAuth(pub User);

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let UserAuth(user) = UserAuth::from_request_parts(parts, state).await?;
        if user.role != "admin" {
            return Err(ApiError(AppError::forbidden("需要管理员权限")).into_response());
        }
        Ok(AdminAuth(user))
    }
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
    pub captcha_id: String,
    pub captcha: String,
}

/// POST /api/auth/login
pub async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> ApiResult<Response> {
    // 验证码
    if !state.cfg.disable_captcha {
        if !state.captcha.verify_and_consume(&req.captcha_id, &req.captcha) {
            return Err(AppError::bad_request("验证码错误").into());
        }
    }
    let user = state.store.get_user_by_username(&req.username)?
        .ok_or_else(|| AppError::bad_request("用户名或密码错误"))?;
    if !crypto::verify_password(&req.password, &user.password_hash) {
        return Err(AppError::bad_request("用户名或密码错误").into());
    }
    if user.status == 0 {
        return Err(AppError::forbidden("账号已禁用").into());
    }
    let sid = state.sessions.create(user.id);
    let body = Json(json!({ "user": public_user(&user) }));
    Ok((
        StatusCode::OK,
        [("set-cookie", format!("relay_session={sid}; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800"))],
        body,
    ).into_response())
}

/// POST /api/auth/logout
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(sid) = session_id(&headers) {
        state.sessions.remove(&sid);
    }
    Ok((
        StatusCode::OK,
        [("set-cookie", "relay_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")],
        Json(json!({ "ok": true })),
    ).into_response())
}

/// GET /api/auth/me
pub async fn me(UserAuth(user): UserAuth) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(public_user(&user)))
}

#[derive(Deserialize)]
pub struct UpdateMeReq {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub password: Option<String>,
}

/// PUT /api/auth/me
pub async fn update_me(State(state): State<AppState>, UserAuth(user): UserAuth, Json(req): Json<UpdateMeReq>) -> ApiResult<Json<serde_json::Value>> {
    let mut u = user;
    if let Some(d) = req.display_name { u.display_name = d; }
    if let Some(e) = req.email { u.email = e; }
    if let Some(p) = req.phone { u.phone = p; }
    u.password_hash = match req.password {
        Some(p) if !p.is_empty() => crypto::hash_password(&p),
        _ => String::new(), // 空 = 不改
    };
    state.store.update_user(&u)?;
    Ok(Json(json!({ "ok": true })))
}

/// 去除敏感字段。
pub fn public_user(u: &User) -> serde_json::Value {
    json!({
        "id": u.id, "username": u.username, "display_name": u.display_name,
        "email": u.email, "phone": u.phone, "role": u.role, "group_id": u.group_id,
        "quota_limit": u.quota_limit, "used_quota": u.used_quota, "status": u.status,
        "created_at": u.created_at,
    })
}
