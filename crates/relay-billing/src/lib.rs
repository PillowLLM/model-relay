//! relay-billing: 定价 / 配额 / 结算 / 兑换码（M1 用字符估算 token，词表计数为 M3）。
use std::sync::Arc;

use relay_core::error::AppError;
use relay_core::*;
use relay_store::Store;

pub struct Billing {
    pub store: Arc<dyn Store>,
}

pub struct CostEstimate {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub upper_cost: f64,
}

impl Billing {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    /// 前置用量估计（字符估算：~4 字符/token）。
    pub fn estimate_cost(&self, req: &CanonicalRequest, group: &Group) -> Result<CostEstimate, AppError> {
        let mut chars: usize = 0;
        if let Some(s) = &req.system { chars += s.chars().count(); }
        for m in &req.messages {
            for p in &m.content {
                if let ContentPart::Text { text } = p { chars += text.chars().count(); }
            }
        }
        let prompt_tokens = (chars as f64 / 4.0).ceil() as u32;
        let pricing = self.store.get_pricing(&req.model)?.unwrap_or_default();
        let cost = calc_cost(prompt_tokens, 0, &pricing, group.quota_multiplier);
        Ok(CostEstimate { prompt_tokens, completion_tokens: 0, upper_cost: cost })
    }

    /// 配额检查：令牌先、用户后；-1 不限。
    pub fn check_quota(&self, token: &Token, user: &User, _upper_cost: f64) -> Result<(), AppError> {
        if token.quota_limit >= 0 && token.used_quota >= token.quota_limit {
            return Err(AppError::rate_limited("令牌额度已用完"));
        }
        if user.quota_limit >= 0 && user.used_quota >= user.quota_limit {
            return Err(AppError::rate_limited("账号额度已用完"));
        }
        Ok(())
    }

    /// 结算：落日志 + 双扣减。
    pub fn record_usage(
        &self,
        token: &Token,
        user: &User,
        group: &Group,
        channel: &Channel,
        model: &str,
        usage: &TokenUsage,
        latency_ms: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let pricing = self.store.get_pricing(model)?.unwrap_or_default();
        let cost = calc_cost(usage.prompt_tokens, usage.completion_tokens, &pricing, group.quota_multiplier);
        let total = (usage.prompt_tokens as i64) + (usage.completion_tokens as i64);
        let log = UsageLog {
            id: 0, token_id: token.id, user_id: user.id, group_id: user.group_id,
            channel_id: channel.id, model: model.into(),
            prompt_tokens: usage.prompt_tokens, completion_tokens: usage.completion_tokens,
            cost, latency_ms, status: status.into(),
            error: error.map(String::from), created_at: relay_core::util::now_str(),
        };
        self.store.insert_usage(&log)?;
        if total > 0 {
            self.store.add_used_quota(user.id, token.id, total)?;
        }
        tracing::info!(token_id = token.id, channel_id = channel.id, model, prompt = usage.prompt_tokens, completion = usage.completion_tokens, cost, "relay_done");
        Ok(())
    }

    /// 兑换码兑换 → 返回到账额度。
    pub fn redeem_code(&self, code: &str, user_id: i64) -> Result<i64, AppError> {
        let c = self.store.get_redeem_code(code)?
            .ok_or_else(|| AppError::bad_request("兑换码不存在"))?;
        if c.status != 0 {
            return Err(AppError::bad_request("兑换码已使用或禁用"));
        }
        if let Some(exp) = &c.expires_at {
            if exp < &relay_core::util::now_str() {
                return Err(AppError::bad_request("兑换码已过期"));
            }
        }
        self.store.redeem(c.id, user_id)?;
        Ok(c.quota)
    }

    /// 加额度（充值到账）。
    pub fn add_quota(&self, user_id: i64, quota: i64) -> Result<(), AppError> {
        // used_quota 减即加额度
        self.store.add_used_quota(user_id, 0, -quota)?;
        Ok(())
    }
}

/// 费用计算：元/1M token * 倍率。
pub fn calc_cost(prompt: u32, completion: u32, p: &Pricing, multiplier: f64) -> f64 {
    (prompt as f64 / 1_000_000.0 * p.prompt_price
        + completion as f64 / 1_000_000.0 * p.completion_price)
        * multiplier
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calc() {
        let p = Pricing { id: 0, model: "x".into(), prompt_price: 1.0, completion_price: 2.0, created_at: String::new() };
        let cost = calc_cost(1_000_000, 500_000, &p, 1.0);
        assert!((cost - 2.0).abs() < 1e-6);
        let cost2 = calc_cost(1_000_000, 0, &p, 0.8);
        assert!((cost2 - 0.8).abs() < 1e-6);
    }
}
