//! relay-gateway: 请求管线（鉴权→限流→配额→选渠道→转发→重试/故障转移→结算→日志）。
mod limit;

pub use limit::RateLimiter;

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{BoxStream, StreamExt};
use futures::Stream;
use http::HeaderMap;
use relay_autofit::{self as autofit, event_to_sse, InboundFormat};
use relay_billing::Billing;
use relay_core::error::AppError;
use relay_core::*;
use relay_health::FailureTracker;
use relay_provider::{map_model, ChannelSelector, ProviderRegistry};
use relay_store::Store;
use serde_json::{json, Value};

pub enum ChatOutcome {
    NonStream(Value),
    Stream(BoxStream<'static, Result<String, AppError>>),
}

pub struct Gateway {
    pub store: Arc<dyn Store>,
    pub billing: Arc<Billing>,
    pub providers: Arc<ProviderRegistry>,
    pub selector: ChannelSelector,
    pub failures: Arc<FailureTracker>,
    pub limits: RateLimiter,
    pub cfg: Config,
}

impl Gateway {
    pub fn new(
        store: Arc<dyn Store>,
        billing: Arc<Billing>,
        providers: Arc<ProviderRegistry>,
        failures: Arc<FailureTracker>,
        cfg: Config,
    ) -> Self {
        Self {
            store, billing, providers,
            selector: ChannelSelector::new(),
            failures,
            limits: RateLimiter::new(),
            cfg,
        }
    }

    /// 主入口。
    pub async fn handle_chat(
        &self,
        path: &str,
        headers: &HeaderMap,
        token_str: Option<String>,
        raw: Value,
    ) -> Result<ChatOutcome, AppError> {
        // 1. 入站识别
        let fmt = autofit::detect_inbound(path, headers, &raw);
        if fmt == InboundFormat::Unknown {
            return Err(AppError::bad_request("无法识别的请求格式"));
        }
        let req = autofit::parse(fmt, &raw)?;

        // 2. 鉴权
        let (token, user, group) = self.authenticate(token_str)?;

        // 3. 限流
        if !self.limits.check(&format!("usr:{}", user.id), self.cfg.rate_limit_per_min) {
            return Err(AppError::rate_limited("请求过于频繁"));
        }

        // 4. 配额预检
        let est = self.billing.estimate_cost(&req, &group)?;
        self.billing.check_quota(&token, &user, est.upper_cost)?;

        // 5. 渠道选择
        let all = self.store.list_channels(true)?;
        let usable = self.providers.usable_channels(&all, user.group_id, &req.model);
        let ordered = self.selector.ordered(&usable);
        if ordered.is_empty() {
            return Err(AppError::bad_request("没有可用渠道或模型不支持"));
        }

        // 6. 转发
        if req.stream {
            self.handle_stream(fmt, &req, &token, &user, &group, ordered).await
        } else {
            self.handle_nonstream(fmt, &req, &token, &user, &group, ordered).await
        }
    }

    async fn handle_nonstream(
        &self,
        fmt: InboundFormat,
        req: &CanonicalRequest,
        token: &Token,
        user: &User,
        group: &Group,
        ordered: Vec<&Channel>,
    ) -> Result<ChatOutcome, AppError> {
        let mut last_err: Option<AppError> = None;
        for (attempt, chan) in ordered.iter().enumerate() {
            if self.failures.is_cooldown(chan.id) {
                continue;
            }
            let mut out_req = req.clone();
            out_req.model = map_model(chan, &req.model);
            let body = match autofit::to_provider(&out_req, &chan.r#type) {
                Ok(b) => b,
                Err(e) => { last_err = Some(e); continue; }
            };
            let provider = match self.providers.get(&chan.r#type) {
                Ok(p) => p,
                Err(e) => { last_err = Some(e); continue; }
            };
            let t0 = Instant::now();
            tracing::info!(token_id = token.id, channel_id = chan.id, model = %req.model, "relay_start");
            match provider.chat(req, body, chan).await {
                Ok(resp) => {
                    self.failures.reset(chan.id);
                    let latency = t0.elapsed().as_millis() as i64;
                    let usage = resp.usage.clone().unwrap_or_else(|| fallback_usage(req, &resp));
                    let _ = self.billing.record_usage(token, user, group, chan, &req.model, &usage, latency, "success", None);
                    return Ok(ChatOutcome::NonStream(autofit::to_client(&resp, fmt)));
                }
                Err(e) => {
                    self.failures.note_failure(chan.id);
                    last_err = Some(e.clone());
                    if !is_retryable(&e) { break; }
                    tokio::time::sleep(Duration::from_millis(200 * (attempt as u64 + 1))).await;
                }
            }
        }
        let err = last_err.unwrap_or_else(|| AppError::internal("所有渠道均失败"));
        // 失败落日志
        let log_chan = ordered.first().copied();
        if let Some(chan) = log_chan {
            let _ = self.billing.record_usage(token, user, group, chan, &req.model, &TokenUsage::default(), 0, "error", Some(err.message()));
        }
        Err(err)
    }

    async fn handle_stream(
        &self,
        fmt: InboundFormat,
        req: &CanonicalRequest,
        token: &Token,
        user: &User,
        group: &Group,
        ordered: Vec<&Channel>,
    ) -> Result<ChatOutcome, AppError> {
        // 流式：取首个非冷却渠道建立连接
        let chan = ordered.iter().find(|c| !self.failures.is_cooldown(c.id)).copied()
            .ok_or_else(|| AppError::bad_request("没有可用渠道"))?;
        let mut out_req = req.clone();
        out_req.model = map_model(chan, &req.model);
        let body = autofit::to_provider(&out_req, &chan.r#type)?;
        let provider = self.providers.get(&chan.r#type)?;
        let id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
        let model_for_stream = req.model.clone();

        let events = provider.chat_stream(req, body, chan).await?;

        // 转换事件流 → 客户端 SSE，并在 usage/done 时结算
        let billing = self.billing.clone();
        let token = token.clone();
        let user = user.clone();
        let group = group.clone();
        let chan = chan.clone();
        let req_model = req.model.clone();
        let req_clone = req.clone();

        let sse: BoxStream<'static, Result<String, AppError>> = StreamConverter {
            inner: events,
            fmt,
            id,
            model: model_for_stream,
            buf: String::new(),
            usage: None,
            done: false,
            billing,
            token,
            user,
            group,
            chan,
            req_model,
            req_clone,
        }
        .boxed();

        Ok(ChatOutcome::Stream(sse))
    }

    /// 令牌鉴权。
    pub fn authenticate(&self, token_str: Option<String>) -> Result<(Token, User, Group), AppError> {
        let plain = token_str.ok_or_else(|| AppError::unauthorized("缺少令牌"))?;
        let hash = relay_core::util::sha256_hex(&plain);
        let token = self.store.get_token_by_hash(&hash)?
            .ok_or_else(|| AppError::unauthorized("令牌无效或已过期"))?;
        if token.status == 0 {
            return Err(AppError::unauthorized("令牌已禁用"));
        }
        if let Some(exp) = &token.expired_at {
            if exp < &relay_core::util::now_str() {
                return Err(AppError::unauthorized("令牌已过期"));
            }
        }
        let user = self.store.get_user_by_id(token.user_id)?
            .ok_or_else(|| AppError::unauthorized("用户不存在"))?;
        if user.status == 0 {
            return Err(AppError::forbidden("账号已禁用"));
        }
        let group = self.store.get_group(user.group_id)?
            .ok_or_else(|| AppError::internal("用户分组不存在"))?;
        Ok((token, user, group))
    }

    /// /v1/models 模型列表（全部启用渠道 models 并集）。
    pub fn list_models(&self) -> Result<Value, AppError> {
        let channels = self.store.list_channels(true)?;
        let mut set = std::collections::BTreeSet::new();
        for c in &channels {
            if c.models.trim().is_empty() {
                continue;
            }
            for m in c.models.split(',') {
                let m = m.trim();
                if !m.is_empty() { set.insert(m.to_string()); }
            }
        }
        let data: Vec<Value> = set.into_iter().map(|m| json!({ "id": m, "object": "model", "owned_by": "relay" })).collect();
        Ok(json!({ "object": "list", "data": data }))
    }

    /// 额度查询（NEWAPI 同款）。
    pub fn billing_subscription(&self, token: &Token) -> Value {
        let hard = if token.quota_limit < 0 { 0.0 } else { token.quota_limit as f64 / 1000.0 };
        json!({
            "object": "billing_subscription",
            "hard_limit_usd": hard,
            "system_hard_limit_usd": hard,
            "soft_limit_usd": hard
        })
    }
}

/// 流式事件 → SSE 转换器。
struct StreamConverter {
    inner: BoxStream<'static, Result<StreamEvent, AppError>>,
    fmt: InboundFormat,
    id: String,
    model: String,
    buf: String,
    usage: Option<TokenUsage>,
    done: bool,
    billing: Arc<Billing>,
    token: Token,
    user: User,
    group: Group,
    chan: Channel,
    req_model: String,
    req_clone: CanonicalRequest,
}

impl Stream for StreamConverter {
    type Item = Result<String, AppError>;

    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        loop {
            if self.done {
                return Poll::Ready(None);
            }
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(None) => {
                    // 上游提前断开：兜底结算
                    self.done = true;
                    let id = self.id.clone();
                    let model = self.model.clone();
                    let content = std::mem::take(&mut self.buf);
                    let usage = self.usage.clone().unwrap_or_else(|| fallback_usage(&self.req_clone, &CanonicalResponse {
                        id, model, content,
                        finish_reason: None, usage: None,
                    }));
                    let _ = self.billing.record_usage(&self.token, &self.user, &self.group, &self.chan, &self.req_model, &usage, 0, "success", None);
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Err(e))) => {
                    let usage = self.usage.clone().unwrap_or_default();
                    let _ = self.billing.record_usage(&self.token, &self.user, &self.group, &self.chan, &self.req_model, &usage, 0, "error", Some(e.message()));
                    self.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(Some(Ok(ev))) => {
                    if let StreamEvent::TextDelta { text } = &ev {
                        self.buf.push_str(text);
                    }
                    if let StreamEvent::Usage { usage } = &ev {
                        self.usage = Some(usage.clone());
                    }
                    if matches!(ev, StreamEvent::Done) {
                        let usage = self.usage.clone().unwrap_or_else(|| fallback_usage(&self.req_clone, &CanonicalResponse {
                            id: self.id.clone(), model: self.model.clone(), content: self.buf.clone(),
                            finish_reason: None, usage: None,
                        }));
                        let _ = self.billing.record_usage(&self.token, &self.user, &self.group, &self.chan, &self.req_model, &usage, 0, "success", None);
                        self.done = true;
                    }
                    if let Some(sse) = event_to_sse(&ev, self.fmt, &self.id, &self.model) {
                        return Poll::Ready(Some(Ok(sse)));
                    }
                    // 该事件无对应 SSE（如 ToolUseDelta），继续拉取下一个
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// 兜底 token 计数（字符/4）。
fn fallback_usage(req: &CanonicalRequest, resp: &CanonicalResponse) -> TokenUsage {
    let mut chars: usize = 0;
    if let Some(s) = &req.system { chars += s.chars().count(); }
    for m in &req.messages {
        for p in &m.content {
            if let ContentPart::Text { text } = p { chars += text.chars().count(); }
        }
    }
    let prompt = (chars as f64 / 4.0).ceil() as u32;
    let completion = (resp.content.chars().count() as f64 / 4.0).ceil() as u32;
    TokenUsage { prompt_tokens: prompt, completion_tokens: completion, total_tokens: prompt + completion }
}

/// 是否可重试：400/401/403/404 不重试。
fn is_retryable(e: &AppError) -> bool {
    !matches!(e, AppError::BadRequest(_) | AppError::Unauthorized(_) | AppError::Forbidden(_) | AppError::NotFound(_))
}
