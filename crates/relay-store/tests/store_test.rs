//! relay-store 集成测试：真实 SqliteStore（临时数据库文件），覆盖 CRUD、
//! 事务兑换、默认分组保护，以及 SQL 注入（参数化查询）防御。
use relay_core::{RedeemCode, User};
use relay_store::{SqliteStore, Store};
use std::path::PathBuf;

fn tmp_db(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("relay-store-test-{}-{}.db", std::process::id(), tag));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{}", p.to_string_lossy(), ext));
    }
    p
}

fn user(username: &str) -> User {
    User {
        id: 0,
        username: username.into(),
        password_hash: "hash".into(),
        display_name: username.into(),
        email: String::new(),
        phone: String::new(),
        role: "user".into(),
        group_id: 1,
        quota_limit: -1,
        used_quota: 0,
        status: 1,
        created_at: String::new(),
    }
}

#[test]
fn init_creates_default_group() {
    let db = tmp_db("init");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();
    let g = store.get_group(1).unwrap().unwrap();
    assert_eq!(g.name, "default");
}

#[test]
fn user_create_and_read_roundtrip() {
    let db = tmp_db("user");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();

    let id = store.create_user(&user("alice")).unwrap();
    assert!(id > 0);

    let by_id = store.get_user_by_id(id).unwrap().unwrap();
    assert_eq!(by_id.username, "alice");

    let by_name = store.get_user_by_username("alice").unwrap().unwrap();
    assert_eq!(by_name.id, id);
}

// ---- INJECTION: SQLi via username must be neutralized by parameterized queries ----

#[test]
fn injection_sql_tautology_does_not_bypass_lookup() {
    let db = tmp_db("sqli");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();
    store.create_user(&user("alice")).unwrap();

    // Classic tautology — with string interpolation this would return every row.
    // With parameterized queries it is treated as a literal username -> no match.
    let evil = "' OR '1'='1";
    let found = store.get_user_by_username(evil).unwrap();
    assert!(found.is_none(), "SQLi tautology must not return any user, got: {found:?}");
}

#[test]
fn injection_semicolon_drop_payload_stored_verbatim_and_table_intact() {
    let db = tmp_db("sqli2");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();

    // A malicious username with a stacked-statement payload must be stored as literal text.
    let evil = "admin'); DROP TABLE users;--";
    let id = store.create_user(&user(evil)).unwrap();
    let back = store.get_user_by_id(id).unwrap().unwrap();
    assert_eq!(back.username, evil);

    // users table still exists and is queryable (other user still there)
    store.create_user(&user("bob")).unwrap();
    let (users, total) = store.list_users(1, 50).unwrap();
    assert_eq!(total, 2, "users table must be intact after stacked-statement payload");
    assert_eq!(users.len(), 2);
}

// ---- transactional redeem ----

#[test]
fn redeem_decreases_quota_and_double_redeem_rejected() {
    let db = tmp_db("redeem");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();

    let mut u = user("carol");
    u.used_quota = 100;
    let uid = store.create_user(&u).unwrap();

    let code = RedeemCode {
        id: 0,
        code: "REDEEM-AAAA-BBBB-CCCC-DDDD".into(),
        quota: 10,
        status: 0,
        used_by: None,
        used_at: None,
        expires_at: None,
        remark: String::new(),
        created_at: String::new(),
    };
    let cid = store.create_redeem(&code).unwrap();

    store.redeem(cid, uid).unwrap();
    let after = store.get_user_by_id(uid).unwrap().unwrap();
    assert_eq!(after.used_quota, 90);

    // second redeem must fail (status flipped to 1)
    assert!(store.redeem(cid, uid).is_err());
}

#[test]
fn default_group_cannot_be_deleted() {
    let db = tmp_db("group");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();
    assert!(store.delete_group(1).is_err());
}

#[test]
fn setting_kv_roundtrip() {
    let db = tmp_db("kv");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();
    store.set_setting("site_name", "my relay").unwrap();
    assert_eq!(store.get_setting("site_name").unwrap().unwrap(), "my relay");
    // overwrite
    store.set_setting("site_name", "renamed").unwrap();
    assert_eq!(store.get_setting("site_name").unwrap().unwrap(), "renamed");
    assert!(store.get_setting("missing").unwrap().is_none());
}

#[test]
fn pagination_lists_total_and_page_size() {
    let db = tmp_db("page");
    let store = SqliteStore::open(&db).unwrap();
    store.init().unwrap();
    for i in 0..3 {
        store.create_user(&user(&format!("u{i}"))).unwrap();
    }
    let (items, total) = store.list_users(1, 2).unwrap();
    assert_eq!(total, 3);
    assert_eq!(items.len(), 2);
    let (items2, _) = store.list_users(2, 2).unwrap();
    assert_eq!(items2.len(), 1);
}
