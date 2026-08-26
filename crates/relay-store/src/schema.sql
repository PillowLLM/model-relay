-- 01 文档建表 SQL（唯一权威 schema）

-- 用户
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  display_name TEXT DEFAULT '',
  email TEXT DEFAULT '',
  phone TEXT DEFAULT '',
  role TEXT NOT NULL DEFAULT 'user',
  group_id INTEGER NOT NULL DEFAULT 1,
  quota_limit INTEGER NOT NULL DEFAULT -1,
  used_quota INTEGER NOT NULL DEFAULT 0,
  status INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

-- 分组
CREATE TABLE IF NOT EXISTS groups (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT UNIQUE NOT NULL,
  quota_multiplier REAL NOT NULL DEFAULT 1.0,
  remark TEXT DEFAULT '',
  created_at TEXT NOT NULL
);

-- 渠道
CREATE TABLE IF NOT EXISTS channels (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  type TEXT NOT NULL,
  base_url TEXT NOT NULL,
  api_key_enc TEXT NOT NULL,
  models TEXT DEFAULT '',
  model_mapping TEXT DEFAULT '{}',
  priority INTEGER NOT NULL DEFAULT 0,
  weight INTEGER NOT NULL DEFAULT 1,
  group_ids TEXT NOT NULL DEFAULT '1',
  status INTEGER NOT NULL DEFAULT 1,
  auto_ban INTEGER NOT NULL DEFAULT 1,
  max_retries INTEGER NOT NULL DEFAULT 1,
  timeout_ms INTEGER NOT NULL DEFAULT 60000,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channels_status ON channels(status, priority DESC, weight DESC);

-- 令牌
CREATE TABLE IF NOT EXISTS tokens (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  key_hash TEXT UNIQUE NOT NULL,
  status INTEGER NOT NULL DEFAULT 1,
  quota_limit INTEGER NOT NULL DEFAULT -1,
  used_quota INTEGER NOT NULL DEFAULT 0,
  expired_at TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tokens_hash ON tokens(key_hash);

-- 兑换码
CREATE TABLE IF NOT EXISTS redeem_codes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  code TEXT UNIQUE NOT NULL,
  quota INTEGER NOT NULL,
  status INTEGER NOT NULL DEFAULT 0,
  used_by INTEGER,
  used_at TEXT,
  expires_at TEXT,
  remark TEXT DEFAULT '',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_redeem_status ON redeem_codes(status);

-- 模型定价
CREATE TABLE IF NOT EXISTS pricing (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  model TEXT UNIQUE NOT NULL,
  prompt_price REAL NOT NULL DEFAULT 0,
  completion_price REAL NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

-- 用量日志
CREATE TABLE IF NOT EXISTS usage_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  token_id INTEGER NOT NULL,
  user_id INTEGER NOT NULL,
  group_id INTEGER NOT NULL DEFAULT 1,
  channel_id INTEGER NOT NULL,
  model TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  cost REAL NOT NULL DEFAULT 0,
  latency_ms INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'success',
  error TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_usage_created ON usage_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_usage_user ON usage_logs(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_token ON usage_logs(token_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_channel ON usage_logs(channel_id, created_at);

-- Token 计数词表文件
CREATE TABLE IF NOT EXISTS tokenizer_files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  family TEXT UNIQUE NOT NULL,
  filename TEXT NOT NULL,
  data BLOB NOT NULL,
  checksum TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- 通知 / 支付配置
CREATE TABLE IF NOT EXISTS notify_config (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT UNIQUE NOT NULL,
  config_enc TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL
);

-- 充值订单
CREATE TABLE IF NOT EXISTS recharge_orders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_no TEXT UNIQUE NOT NULL,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  amount REAL NOT NULL,
  quota INTEGER NOT NULL,
  pay_method TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  qr_url TEXT,
  paid_at TEXT,
  confirmed_by INTEGER,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_recharge_user ON recharge_orders(user_id, created_at);

-- 通知发送记录
CREATE TABLE IF NOT EXISTS notification_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER,
  channel TEXT NOT NULL,
  template TEXT NOT NULL,
  to_addr TEXT,
  status TEXT NOT NULL,
  error TEXT,
  created_at TEXT NOT NULL
);

-- 设置 KV
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
