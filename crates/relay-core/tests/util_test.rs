//! relay-core util 公共工具函数测试（集成测试，仅依赖公共 API）。
use relay_core::util::{
    base64_encode, gen_redeem_code, gen_token, mask_key, parse_group_ids, sha256_hex,
};

#[test]
fn gen_token_format_and_charset() {
    let t = gen_token();
    assert!(t.starts_with("sk-"));
    assert_eq!(t.len(), 27); // "sk-" + 24
    assert!(t[3..].chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn gen_redeem_code_format() {
    let r = gen_redeem_code();
    assert!(r.starts_with("REDEEM-"));
    assert_eq!(r.matches('-').count(), 4);
}

#[test]
fn sha256_known_vector() {
    // FIPS 180-2 example: sha256("abc")
    assert_eq!(
        sha256_hex("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(sha256_hex(""), sha256_hex(""));
}

#[test]
fn parse_group_ids_skips_junk_and_injection_chars() {
    assert_eq!(parse_group_ids("1, 2,3"), vec![1, 2, 3]);
    // INJECTION: the "2; DROP TABLE users;--" chunk is not a valid i64 and is dropped
    // entirely (never executed), so it does NOT contribute a "2".
    assert_eq!(parse_group_ids("1,abc,2; DROP TABLE users;--,3"), vec![1, 3]);
    assert!(parse_group_ids("").is_empty());
}

#[test]
fn mask_key_short_and_long() {
    assert_eq!(mask_key("ab"), "****");
    let m = mask_key("sk-1234567890abcdef");
    assert!(m.contains("****"));
    assert!(m.ends_with("cdef"));
    assert!(m.starts_with("sk-1"));
}

#[test]
fn base64_roundtrip() {
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
}
