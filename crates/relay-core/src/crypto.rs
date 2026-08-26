use crate::error::AppError;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;

/// AES-256-GCM 加密：base64(nonce12) + "." + base64(ciphertext)。
pub fn encrypt(key: &[u8; 32], plain: &str) -> String {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain.as_bytes())
        .expect("encrypt failed");
    let b64 = base64::engine::general_purpose::STANDARD;
    format!(
        "{}.{}",
        b64.encode(nonce_bytes),
        b64.encode(ct)
    )
}

/// 解密；篡改/错误返回 Internal。
pub fn decrypt(key: &[u8; 32], enc: &str) -> Result<String, AppError> {
    let (nonce_b64, ct_b64) = enc
        .split_once('.')
        .ok_or_else(|| AppError::internal("密文格式错误"))?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let nonce_bytes = b64
        .decode(nonce_b64)
        .map_err(|e| AppError::internal(format!("nonce 解码失败: {e}")))?;
    let ct = b64
        .decode(ct_b64)
        .map_err(|e| AppError::internal(format!("密文解码失败: {e}")))?;
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(&nonce_bytes);
    let pt = cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|_| AppError::internal("解密失败（密钥不符或密文被篡改）"))?;
    String::from_utf8(pt).map_err(|e| AppError::internal(format!("明文非 UTF-8: {e}")))
}

/// scrypt 密码哈希：base64(salt16 + hash64)。
pub fn hash_password(pw: &str) -> String {
    use scrypt::scrypt;
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut hash = [0u8; 64];
    let params = scrypt::Params::new(14, 8, 1, 64).expect("scrypt params");
    scrypt(pw.as_bytes(), &salt, &params, &mut hash).expect("scrypt");
    let mut combined = Vec::with_capacity(80);
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&hash);
    base64::engine::general_purpose::STANDARD.encode(combined)
}

/// 校验密码。
pub fn verify_password(pw: &str, stored: &str) -> bool {
    use scrypt::scrypt;
    let b64 = base64::engine::general_purpose::STANDARD;
    let combined = match b64.decode(stored) {
        Ok(c) if c.len() == 80 => c,
        _ => return false,
    };
    let salt = &combined[..16];
    let params = scrypt::Params::new(14, 8, 1, 64).expect("scrypt params");
    let mut hash = [0u8; 64];
    if scrypt(pw.as_bytes(), salt, &params, &mut hash).is_err() {
        return false;
    }
    hash[..] == combined[16..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_roundtrip() {
        let key = [42u8; 32];
        let enc = encrypt(&key, "sk-secret-123");
        let dec = decrypt(&key, &enc).unwrap();
        assert_eq!(dec, "sk-secret-123");
        // 篡改
        let mut bad = enc.clone();
        bad.replace_range(0..1, "0");
        assert!(decrypt(&key, &bad).is_err());
    }

    #[test]
    fn password_roundtrip() {
        let h = hash_password("hunter2");
        assert!(verify_password("hunter2", &h));
        assert!(!verify_password("wrong", &h));
    }
}
