use rand::Rng;
use sha2::{Digest, Sha256};
use base64::Engine;

/// "YYYY-MM-DD HH:MM:SS"（chrono::Local）。
pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// "sk-" + 24 位随机 [a-zA-Z0-9]。
pub fn gen_token() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let s: String = (0..24)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect();
    format!("sk-{s}")
}

/// "REDEEM-" + 4×4 大写字母数字（用 - 分隔）。
pub fn gen_redeem_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let mut group = || -> String {
        (0..4)
            .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
            .collect()
    };
    format!("REDEEM-{}-{}-{}-{}", group(), group(), group(), group())
}

/// "R" + 时间戳 + 6 位随机。
pub fn gen_order_no() -> String {
    let ts = chrono::Local::now().timestamp();
    let n: u32 = rand::thread_rng().gen_range(0..1_000_000);
    format!("R{ts}{n:06}")
}

pub fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// "1,2,3" → [1,2,3]
pub fn parse_group_ids(s: &str) -> Vec<i64> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<i64>().ok())
        .collect()
}

/// 脱敏：保留前缀 + 末尾 4 位。
pub fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".into();
    }
    let tail = &key[key.len() - 4..];
    format!("{}****{}", &key[..4.min(key.len())], tail)
}

/// base64 标准编码（供 captcha 等使用）。
pub fn base64_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let t = gen_token();
        assert!(t.starts_with("sk-") && t.len() == 27);
        let r = gen_redeem_code();
        assert!(r.starts_with("REDEEM-"));
        assert_eq!(r.matches('-').count(), 4);
        assert_eq!(sha256_hex("abc"), sha256_hex("abc"));
        assert_eq!(parse_group_ids("1, 2,3"), vec![1, 2, 3]);
    }
}
