//! relay-store: DAO trait + SqliteStore（单连接 + Mutex）。
use std::sync::Mutex;

use relay_core::error::AppError;
use relay_core::*;
use rusqlite::{params, Connection, OptionalExtension};

pub const SCHEMA: &str = include_str!("schema.sql");

pub trait Store: Send + Sync {
    fn init(&self) -> Result<(), AppError>;
    // 用户/分组
    fn get_user_by_id(&self, id: i64) -> Result<Option<User>, AppError>;
    fn get_user_by_username(&self, name: &str) -> Result<Option<User>, AppError>;
    fn create_user(&self, u: &User) -> Result<i64, AppError>;
    fn update_user(&self, u: &User) -> Result<(), AppError>;
    fn delete_user(&self, id: i64) -> Result<(), AppError>;
    fn list_users(&self, page: u32, size: u32) -> Result<(Vec<User>, u32), AppError>;
    fn add_used_quota(&self, user_id: i64, token_id: i64, delta: i64) -> Result<(), AppError>;
    fn get_group(&self, id: i64) -> Result<Option<Group>, AppError>;
    fn list_groups(&self) -> Result<Vec<Group>, AppError>;
    fn upsert_group(&self, g: &Group) -> Result<i64, AppError>;
    fn delete_group(&self, id: i64) -> Result<(), AppError>;
    // 渠道
    fn list_channels(&self, enabled_only: bool) -> Result<Vec<Channel>, AppError>;
    fn get_channel(&self, id: i64) -> Result<Option<Channel>, AppError>;
    fn upsert_channel(&self, c: &Channel) -> Result<i64, AppError>;
    fn delete_channel(&self, id: i64) -> Result<(), AppError>;
    fn update_channel_status(&self, id: i64, status: i32) -> Result<(), AppError>;
    // 令牌
    fn get_token_by_hash(&self, hash: &str) -> Result<Option<Token>, AppError>;
    fn list_tokens(&self, user_id: Option<i64>, page: u32, size: u32) -> Result<(Vec<Token>, u32), AppError>;
    fn create_token(&self, t: &Token) -> Result<i64, AppError>;
    fn update_token(&self, t: &Token) -> Result<(), AppError>;
    fn delete_token(&self, id: i64) -> Result<(), AppError>;
    fn update_token_status(&self, id: i64, status: i32) -> Result<(), AppError>;
    // 兑换码
    fn get_redeem_code(&self, code: &str) -> Result<Option<RedeemCode>, AppError>;
    fn list_redeem(&self, status: Option<i32>, page: u32, size: u32) -> Result<(Vec<RedeemCode>, u32), AppError>;
    fn create_redeem(&self, c: &RedeemCode) -> Result<i64, AppError>;
    fn redeem(&self, code_id: i64, user_id: i64) -> Result<(), AppError>;
    fn update_redeem_status(&self, id: i64, status: i32) -> Result<(), AppError>;
    // 用量
    fn insert_usage(&self, u: &UsageLog) -> Result<(), AppError>;
    fn query_usage(&self, filter: &UsageFilter) -> Result<(Vec<UsageLog>, u32), AppError>;
    fn usage_stats(&self, days: u32) -> Result<StatSummary, AppError>;
    // 定价
    fn list_pricing(&self) -> Result<Vec<Pricing>, AppError>;
    fn upsert_pricing(&self, p: &Pricing) -> Result<(), AppError>;
    fn get_pricing(&self, model: &str) -> Result<Option<Pricing>, AppError>;
    fn delete_pricing(&self, id: i64) -> Result<(), AppError>;
    // 设置 KV
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError>;
    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError>;
    // 自检
    fn integrity_check(&self) -> Result<String, AppError>;
}

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: &std::path::Path) -> Result<Self, AppError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| AppError::internal(format!("创建数据目录失败: {e}")))?;
        }
        let conn = Connection::open(path).map_err(|e| AppError::internal(format!("打开数据库失败: {e}")))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(rusql_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(rusql_err)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

fn rusql_err(e: rusqlite::Error) -> AppError {
    AppError::internal(format!("数据库错误: {e}"))
}

fn now() -> String {
    relay_core::util::now_str()
}

impl Store for SqliteStore {
    fn init(&self) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(SCHEMA).map_err(rusql_err)?;
        // 默认分组
        conn.execute(
            "INSERT OR IGNORE INTO groups (id, name, quota_multiplier, remark, created_at) VALUES (1, 'default', 1.0, '', ?1)",
            params![now()],
        ).map_err(rusql_err)?;
        Ok(())
    }

    fn get_user_by_id(&self, id: i64) -> Result<Option<User>, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT * FROM users WHERE id=?1", params![id], row_user)
            .optional()
            .map_err(rusql_err)
    }
    fn get_user_by_username(&self, name: &str) -> Result<Option<User>, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT * FROM users WHERE username=?1", params![name], row_user)
            .optional()
            .map_err(rusql_err)
    }
    fn create_user(&self, u: &User) -> Result<i64, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username,password_hash,display_name,email,phone,role,group_id,quota_limit,used_quota,status,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![u.username, u.password_hash, u.display_name, u.email, u.phone, u.role, u.group_id, u.quota_limit, u.used_quota, u.status, u.created_at],
        ).map_err(rusql_err)?;
        Ok(conn.last_insert_rowid())
    }
    fn update_user(&self, u: &User) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET display_name=?1,email=?2,phone=?3,role=?4,group_id=?5,quota_limit=?6,status=?7,password_hash=CASE WHEN ?8='' THEN password_hash ELSE ?8 END WHERE id=?9",
            params![u.display_name, u.email, u.phone, u.role, u.group_id, u.quota_limit, u.status, u.password_hash, u.id],
        ).map_err(rusql_err)?;
        Ok(())
    }
    fn delete_user(&self, id: i64) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM users WHERE id=?1", params![id]).map_err(rusql_err)?;
        Ok(())
    }
    fn list_users(&self, page: u32, size: u32) -> Result<(Vec<User>, u32), AppError> {
        let conn = self.conn.lock().unwrap();
        let total: u32 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).map_err(rusql_err)?;
        let offset = (page.saturating_sub(1)) * size;
        let mut stmt = conn.prepare("SELECT * FROM users ORDER BY id DESC LIMIT ?1 OFFSET ?2").map_err(rusql_err)?;
        let items = stmt.query_map(params![size, offset], row_user).map_err(rusql_err)?.filter_map(|r| r.ok()).collect();
        Ok((items, total))
    }
    fn add_used_quota(&self, user_id: i64, token_id: i64, delta: i64) -> Result<(), AppError> {
        let mut guard = self.conn.lock().unwrap();
        let tx = guard.transaction().map_err(rusql_err)?;
        tx.execute("UPDATE users SET used_quota=used_quota+?1 WHERE id=?2", params![delta, user_id]).map_err(rusql_err)?;
        tx.execute("UPDATE tokens SET used_quota=used_quota+?1 WHERE id=?2", params![delta, token_id]).map_err(rusql_err)?;
        tx.commit().map_err(rusql_err)?;
        Ok(())
    }
    fn get_group(&self, id: i64) -> Result<Option<Group>, AppError> {
        self.conn.lock().unwrap().query_row("SELECT * FROM groups WHERE id=?1", params![id], row_group).optional().map_err(rusql_err)
    }
    fn list_groups(&self) -> Result<Vec<Group>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM groups ORDER BY id").map_err(rusql_err)?;
        let items = stmt.query_map([], row_group).map_err(rusql_err)?.filter_map(|r| r.ok()).collect();
        Ok(items)
    }
    fn upsert_group(&self, g: &Group) -> Result<i64, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO groups (id,name,quota_multiplier,remark,created_at) VALUES (?1,?2,?3,?4,?5) \
            ON CONFLICT(id) DO UPDATE SET name=?2,quota_multiplier=?3,remark=?4",
            params![if g.id>0 {Some(g.id)} else {None}, g.name, g.quota_multiplier, g.remark, if g.created_at.is_empty(){now()} else {g.created_at.clone()}],
        ).map_err(rusql_err)?;
        Ok(conn.last_insert_rowid())
    }
    fn delete_group(&self, id: i64) -> Result<(), AppError> {
        if id == 1 { return Err(AppError::bad_request("默认分组不可删除")); }
        self.conn.lock().unwrap().execute("DELETE FROM groups WHERE id=?1", params![id]).map_err(rusql_err)?;
        Ok(())
    }

    fn list_channels(&self, enabled_only: bool) -> Result<Vec<Channel>, AppError> {
        let conn = self.conn.lock().unwrap();
        let sql = if enabled_only { "SELECT * FROM channels WHERE status=1 ORDER BY priority DESC, weight DESC" } else { "SELECT * FROM channels ORDER BY id DESC" };
        let mut stmt = conn.prepare(sql).map_err(rusql_err)?;
        let items = stmt.query_map([], row_channel).map_err(rusql_err)?.filter_map(|r| r.ok()).collect();
        Ok(items)
    }
    fn get_channel(&self, id: i64) -> Result<Option<Channel>, AppError> {
        self.conn.lock().unwrap().query_row("SELECT * FROM channels WHERE id=?1", params![id], row_channel).optional().map_err(rusql_err)
    }
    fn upsert_channel(&self, c: &Channel) -> Result<i64, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO channels (name,type,base_url,api_key_enc,models,model_mapping,priority,weight,group_ids,status,auto_ban,max_retries,timeout_ms,created_at) \
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) \
            ON CONFLICT(id) DO UPDATE SET name=?1,type=?2,base_url=?3,api_key_enc=CASE WHEN ?4='' THEN api_key_enc ELSE ?4 END,models=?5,model_mapping=?6,priority=?7,weight=?8,group_ids=?9,status=?10,auto_ban=?11,max_retries=?12,timeout_ms=?13",
            params![c.name,c.r#type,c.base_url,c.api_key_enc,c.models,c.model_mapping,c.priority,c.weight,c.group_ids,c.status,c.auto_ban,c.max_retries,c.timeout_ms, if c.created_at.is_empty(){now()} else {c.created_at.clone()}],
        ).map_err(rusql_err)?;
        Ok(conn.last_insert_rowid())
    }
    fn delete_channel(&self, id: i64) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("DELETE FROM channels WHERE id=?1", params![id]).map_err(rusql_err)?;
        Ok(())
    }
    fn update_channel_status(&self, id: i64, status: i32) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("UPDATE channels SET status=?1 WHERE id=?2", params![status, id]).map_err(rusql_err)?;
        Ok(())
    }

    fn get_token_by_hash(&self, hash: &str) -> Result<Option<Token>, AppError> {
        self.conn.lock().unwrap().query_row("SELECT * FROM tokens WHERE key_hash=?1", params![hash], row_token).optional().map_err(rusql_err)
    }
    fn list_tokens(&self, user_id: Option<i64>, page: u32, size: u32) -> Result<(Vec<Token>, u32), AppError> {
        let conn = self.conn.lock().unwrap();
        let (count_sql, list_sql) = match user_id {
            Some(uid) => ("SELECT COUNT(*) FROM tokens WHERE user_id=?1", "SELECT * FROM tokens WHERE user_id=?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3"),
            None => ("SELECT COUNT(*) FROM tokens", "SELECT * FROM tokens ORDER BY id DESC LIMIT ?1 OFFSET ?2"),
        };
        let total: u32 = if let Some(uid) = user_id { conn.query_row(count_sql, params![uid], |r| r.get(0)) } else { conn.query_row(count_sql, [], |r| r.get(0)) }.map_err(rusql_err)?;
        let offset = page.saturating_sub(1) * size;
        let mut stmt = conn.prepare(list_sql).map_err(rusql_err)?;
        let items = if let Some(uid) = user_id {
            stmt.query_map(params![uid, size, offset], row_token).map_err(rusql_err)?.filter_map(|r| r.ok()).collect()
        } else {
            stmt.query_map(params![size, offset], row_token).map_err(rusql_err)?.filter_map(|r| r.ok()).collect()
        };
        Ok((items, total))
    }
    fn create_token(&self, t: &Token) -> Result<i64, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO tokens (user_id,name,key_hash,status,quota_limit,used_quota,expired_at,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![t.user_id,t.name,t.key_hash,t.status,t.quota_limit,t.used_quota,t.expired_at,t.created_at]).map_err(rusql_err)?;
        Ok(conn.last_insert_rowid())
    }
    fn update_token(&self, t: &Token) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("UPDATE tokens SET name=?1,quota_limit=?2,expired_at=?3 WHERE id=?4", params![t.name,t.quota_limit,t.expired_at,t.id]).map_err(rusql_err)?;
        Ok(())
    }
    fn delete_token(&self, id: i64) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("DELETE FROM tokens WHERE id=?1", params![id]).map_err(rusql_err)?;
        Ok(())
    }
    fn update_token_status(&self, id: i64, status: i32) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("UPDATE tokens SET status=?1 WHERE id=?2", params![status, id]).map_err(rusql_err)?;
        Ok(())
    }

    fn get_redeem_code(&self, code: &str) -> Result<Option<RedeemCode>, AppError> {
        self.conn.lock().unwrap().query_row("SELECT * FROM redeem_codes WHERE code=?1", params![code], row_redeem).optional().map_err(rusql_err)
    }
    fn list_redeem(&self, status: Option<i32>, page: u32, size: u32) -> Result<(Vec<RedeemCode>, u32), AppError> {
        let conn = self.conn.lock().unwrap();
        let total: u32 = match status { Some(s) => conn.query_row("SELECT COUNT(*) FROM redeem_codes WHERE status=?1", params![s], |r| r.get(0)), None => conn.query_row("SELECT COUNT(*) FROM redeem_codes", [], |r| r.get(0)) }.map_err(rusql_err)?;
        let offset = page.saturating_sub(1) * size;
        let mut stmt = if let Some(s) = status { conn.prepare("SELECT * FROM redeem_codes WHERE status=?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3").map_err(rusql_err)? } else { conn.prepare("SELECT * FROM redeem_codes ORDER BY id DESC LIMIT ?1 OFFSET ?2").map_err(rusql_err)? };
        let items = if let Some(s) = status { stmt.query_map(params![s, size, offset], row_redeem).map_err(rusql_err)?.filter_map(|r| r.ok()).collect() } else { stmt.query_map(params![size, offset], row_redeem).map_err(rusql_err)?.filter_map(|r| r.ok()).collect() };
        Ok((items, total))
    }
    fn create_redeem(&self, c: &RedeemCode) -> Result<i64, AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO redeem_codes (code,quota,status,remark,created_at) VALUES (?1,?2,?3,?4,?5)", params![c.code,c.quota,c.status,c.remark,c.created_at]).map_err(rusql_err)?;
        Ok(conn.last_insert_rowid())
    }
    fn redeem(&self, code_id: i64, user_id: i64) -> Result<(), AppError> {
        let mut guard = self.conn.lock().unwrap();
        let tx = guard.transaction().map_err(rusql_err)?;
        let affected = tx.execute("UPDATE redeem_codes SET status=1,used_by=?1,used_at=?2 WHERE id=?3 AND status=0", params![user_id, now(), code_id]).map_err(rusql_err)?;
        if affected == 0 { return Err(AppError::bad_request("兑换码已被使用或禁用")); }
        let quota: i64 = tx.query_row("SELECT quota FROM redeem_codes WHERE id=?1", params![code_id], |r| r.get(0)).map_err(rusql_err)?;
        tx.execute("UPDATE users SET used_quota=used_quota-?1 WHERE id=?2", params![quota, user_id]).map_err(rusql_err)?;
        tx.commit().map_err(rusql_err)?;
        Ok(())
    }
    fn update_redeem_status(&self, id: i64, status: i32) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("UPDATE redeem_codes SET status=?1 WHERE id=?2", params![status, id]).map_err(rusql_err)?;
        Ok(())
    }

    fn insert_usage(&self, u: &UsageLog) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("INSERT INTO usage_logs (token_id,user_id,group_id,channel_id,model,prompt_tokens,completion_tokens,cost,latency_ms,status,error,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![u.token_id,u.user_id,u.group_id,u.channel_id,u.model,u.prompt_tokens,u.completion_tokens,u.cost,u.latency_ms,u.status,u.error,u.created_at]).map_err(rusql_err)?;
        Ok(())
    }
    fn query_usage(&self, filter: &UsageFilter) -> Result<(Vec<UsageLog>, u32), AppError> {
        let conn = self.conn.lock().unwrap();
        let mut where_clauses: Vec<&'static str> = Vec::new();
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(uid) = filter.user_id { where_clauses.push("user_id=?"); p.push(Box::new(uid)); }
        if let Some(cid) = filter.channel_id { where_clauses.push("channel_id=?"); p.push(Box::new(cid)); }
        if let Some(m) = &filter.model { where_clauses.push("model=?"); p.push(Box::new(m.clone())); }
        if let Some(s) = &filter.start { where_clauses.push("created_at>=?"); p.push(Box::new(s.clone())); }
        if let Some(e) = &filter.end { where_clauses.push("created_at<=?"); p.push(Box::new(e.clone())); }
        let wc = if where_clauses.is_empty() { String::new() } else { format!("WHERE {}", where_clauses.join(" AND ")) };
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        let total: u32 = conn.query_row(&format!("SELECT COUNT(*) FROM usage_logs {wc}"), rusqlite::params_from_iter(refs.iter().copied()), |r| r.get(0)).map_err(rusql_err)?;
        let size = if filter.page_size == 0 { 20 } else { filter.page_size } as i64;
        let offset = (filter.page.saturating_sub(1) as i64) * size;
        let mut p2: Vec<Box<dyn rusqlite::ToSql>> = p;
        p2.push(Box::new(size));
        p2.push(Box::new(offset));
        let refs2: Vec<&dyn rusqlite::ToSql> = p2.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&format!("SELECT * FROM usage_logs {wc} ORDER BY id DESC LIMIT ? OFFSET ?")).map_err(rusql_err)?;
        let items = stmt.query_map(rusqlite::params_from_iter(refs2.iter().copied()), row_usage).map_err(rusql_err)?.filter_map(|r| r.ok()).collect();
        Ok((items, total))
    }
    fn usage_stats(&self, days: u32) -> Result<StatSummary, AppError> {
        let conn = self.conn.lock().unwrap();
        let since = chrono::Local::now().naive_local().date().and_hms_opt(0, 0, 0).unwrap()
            .checked_sub_days(chrono::Days::new(days as u64)).unwrap()
            .format("%Y-%m-%d 00:00:00").to_string();
        let row: (i64, f64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(prompt_tokens+completion_tokens),0), COALESCE(SUM(cost),0), COUNT(*) FROM usage_logs WHERE created_at>=?1",
            params![&since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).map_err(rusql_err)?;
        Ok(StatSummary {
            total_tokens: row.0, total_cost: row.1, requests: row.2,
            by_model: agg_by(&conn, &since, "model")?,
            by_day: agg_by(&conn, &since, "date(created_at)")?,
            by_channel: agg_by(&conn, &since, "'channel_'||channel_id")?,
        })
    }
    fn list_pricing(&self) -> Result<Vec<Pricing>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM pricing ORDER BY model").map_err(rusql_err)?;
        let items = stmt.query_map([], row_pricing).map_err(rusql_err)?.filter_map(|r| r.ok()).collect();
        Ok(items)
    }
    fn upsert_pricing(&self, p: &Pricing) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("INSERT INTO pricing (model,prompt_price,completion_price,created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(model) DO UPDATE SET prompt_price=?2,completion_price=?3",
            params![p.model, p.prompt_price, p.completion_price, if p.created_at.is_empty(){now()} else {p.created_at.clone()}]).map_err(rusql_err)?;
        Ok(())
    }
    fn get_pricing(&self, model: &str) -> Result<Option<Pricing>, AppError> {
        self.conn.lock().unwrap().query_row("SELECT * FROM pricing WHERE model=?1", params![model], row_pricing).optional().map_err(rusql_err)
    }
    fn delete_pricing(&self, id: i64) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("DELETE FROM pricing WHERE id=?1", params![id]).map_err(rusql_err)?;
        Ok(())
    }
    fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        self.conn.lock().unwrap().query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get::<_,String>(0)).optional().map_err(rusql_err)
    }
    fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn.lock().unwrap().execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=?2", params![key, value]).map_err(rusql_err)?;
        Ok(())
    }
    fn integrity_check(&self) -> Result<String, AppError> {
        self.conn.lock().unwrap().query_row("PRAGMA integrity_check", [], |r| r.get::<_,String>(0)).map_err(rusql_err)
    }
}

// ---- row mappers ----
fn agg_by(conn: &Connection, since: &str, group: &str) -> Result<Vec<ByGroup>, AppError> {
    let sql = format!(
        "SELECT {group} AS k, COALESCE(SUM(prompt_tokens+completion_tokens),0), COALESCE(SUM(cost),0) \
         FROM usage_logs WHERE created_at>=?1 GROUP BY {group} ORDER BY 2 DESC LIMIT 20"
    );
    let mut stmt = conn.prepare(&sql).map_err(rusql_err)?;
    let rows = stmt.query_map(params![since], |r| Ok(ByGroup {
        key: r.get(0)?, tokens: r.get(1)?, cost: r.get(2)?,
    })).map_err(rusql_err)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn row_user(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get("id")?, username: r.get("username")?, password_hash: r.get("password_hash")?,
        display_name: r.get("display_name")?, email: r.get("email")?, phone: r.get("phone")?,
        role: r.get("role")?, group_id: r.get("group_id")?, quota_limit: r.get("quota_limit")?,
        used_quota: r.get("used_quota")?, status: r.get("status")?, created_at: r.get("created_at")?,
    })
}
fn row_group(r: &rusqlite::Row<'_>) -> rusqlite::Result<Group> {
    Ok(Group { id: r.get("id")?, name: r.get("name")?, quota_multiplier: r.get("quota_multiplier")?, remark: r.get("remark")?, created_at: r.get("created_at").unwrap_or_default() })
}
fn row_channel(r: &rusqlite::Row<'_>) -> rusqlite::Result<Channel> {
    Ok(Channel {
        id: r.get("id")?, name: r.get("name")?, r#type: r.get("type")?, base_url: r.get("base_url")?,
        api_key_enc: r.get("api_key_enc")?, models: r.get("models")?, model_mapping: r.get("model_mapping")?,
        priority: r.get("priority")?, weight: r.get("weight")?, group_ids: r.get("group_ids")?,
        status: r.get("status")?, auto_ban: r.get("auto_ban")?, max_retries: r.get("max_retries")?,
        timeout_ms: r.get("timeout_ms")?, created_at: r.get("created_at")?,
    })
}
fn row_token(r: &rusqlite::Row<'_>) -> rusqlite::Result<Token> {
    Ok(Token { id: r.get("id")?, user_id: r.get("user_id")?, name: r.get("name")?, key_hash: r.get("key_hash")?,
        status: r.get("status")?, quota_limit: r.get("quota_limit")?, used_quota: r.get("used_quota")?,
        expired_at: r.get("expired_at")?, created_at: r.get("created_at")? })
}
fn row_redeem(r: &rusqlite::Row<'_>) -> rusqlite::Result<RedeemCode> {
    Ok(RedeemCode { id: r.get("id")?, code: r.get("code")?, quota: r.get("quota")?, status: r.get("status")?,
        used_by: r.get("used_by")?, used_at: r.get("used_at")?, expires_at: r.get("expires_at")?,
        remark: r.get("remark")?, created_at: r.get("created_at")? })
}
fn row_usage(r: &rusqlite::Row<'_>) -> rusqlite::Result<UsageLog> {
    Ok(UsageLog { id: r.get("id")?, token_id: r.get("token_id")?, user_id: r.get("user_id")?, group_id: r.get("group_id")?,
        channel_id: r.get("channel_id")?, model: r.get("model")?, prompt_tokens: r.get("prompt_tokens")?,
        completion_tokens: r.get("completion_tokens")?, cost: r.get("cost")?, latency_ms: r.get("latency_ms")?,
        status: r.get("status")?, error: r.get("error")?, created_at: r.get("created_at")? })
}
fn row_pricing(r: &rusqlite::Row<'_>) -> rusqlite::Result<Pricing> {
    Ok(Pricing { id: r.get("id")?, model: r.get("model")?, prompt_price: r.get("prompt_price")?,
        completion_price: r.get("completion_price")?, created_at: r.get("created_at")? })
}
