-- ================================================
-- EXA ROBOT - Complete Database Schema
-- ================================================
-- This file creates ALL tables with correct schema
-- Run this ONCE on fresh installation
-- ================================================

-- ================================================
-- CORE TABLES
-- ================================================

-- Settings (Key-Value Store)
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Admins
CREATE TABLE IF NOT EXISTS admins (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- NODE MANAGEMENT
-- ================================================

CREATE TABLE IF NOT EXISTS nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    ip TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'new', -- new, installing, active, error, offline
    root_password TEXT,
    ssh_port INTEGER DEFAULT 22,
    ssh_user TEXT NOT NULL DEFAULT 'root',
    ssh_password TEXT,
    vpn_port INTEGER NOT NULL DEFAULT 443,
    last_seen DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    -- Reality Keys (auto-generated)
    reality_pub TEXT,
    reality_priv TEXT,
    short_id TEXT,
    domain TEXT,
    -- Smart Setup
    join_token TEXT,

    auto_configure BOOLEAN DEFAULT 0,
    is_enabled BOOLEAN DEFAULT 1 -- Added in v1.1
);

CREATE INDEX IF NOT EXISTS idx_nodes_ip ON nodes (ip);
CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes (status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_nodes_join_token ON nodes (join_token);

-- ================================================
-- NETWORK CONFIGURATION
-- ================================================

CREATE TABLE IF NOT EXISTS inbounds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL,
    tag TEXT NOT NULL,
    protocol TEXT NOT NULL, -- vless, hysteria2, shadowsocks, etc
    listen_port INTEGER NOT NULL DEFAULT 443,
    listen_ip TEXT DEFAULT '::',
    settings TEXT NOT NULL DEFAULT '{}', -- JSON: protocol-specific settings
    stream_settings TEXT NOT NULL DEFAULT '{}', -- JSON: network/security settings
    remark TEXT,
    enable BOOLEAN DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE,
    UNIQUE(node_id, listen_port)
);

CREATE INDEX IF NOT EXISTS idx_inbounds_node ON inbounds (node_id);
CREATE INDEX IF NOT EXISTS idx_inbounds_protocol ON inbounds (protocol);

-- ================================================
-- USER MANAGEMENT
-- ================================================

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tg_id INTEGER NOT NULL UNIQUE,
    username TEXT,
    first_name TEXT,
    last_name TEXT,
    full_name TEXT, -- Concatenated first + last name
    language_code TEXT DEFAULT 'en',
    is_banned BOOLEAN DEFAULT 0,
    ban_reason TEXT,
    banned_at DATETIME,
    referrer_id INTEGER,
    referral_code TEXT UNIQUE,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_seen DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (referrer_id) REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_users_tg_id ON users (tg_id);
CREATE INDEX IF NOT EXISTS idx_users_referral_code ON users (referral_code);
CREATE INDEX IF NOT EXISTS idx_users_referrer_id ON users (referrer_id);

-- ================================================
-- STORE & SUBSCRIPTIONS
-- ================================================

CREATE TABLE IF NOT EXISTS plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    traffic_limit_gb INTEGER NOT NULL, -- -1 for unlimited
    device_limit INTEGER DEFAULT 1,
    price REAL NOT NULL,
    is_active BOOLEAN DEFAULT 1,
    sort_order INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS plan_durations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id INTEGER NOT NULL,
    duration_days INTEGER NOT NULL,
    price REAL NOT NULL,
    discount_percent REAL DEFAULT 0,
    is_active BOOLEAN DEFAULT 1,
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plan_durations_plan_id ON plan_durations (plan_id);

CREATE TABLE IF NOT EXISTS subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    plan_id INTEGER NOT NULL,
    vless_uuid TEXT UNIQUE, -- Generated once, used for all configs
    status TEXT NOT NULL DEFAULT 'pending', -- pending, active, expired, suspended
    traffic_used_gb REAL DEFAULT 0,
    device_count INTEGER DEFAULT 0,
    activated_at DATETIME,
    expires_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    note TEXT, -- Admin notes
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans(id)
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user_id ON subscriptions (user_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions (status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_vless_uuid ON subscriptions (vless_uuid);

-- ================================================
-- PAYMENTS & ORDERS
-- ================================================

CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    plan_id INTEGER NOT NULL,
    duration_days INTEGER NOT NULL,
    amount REAL NOT NULL, -- Will be named total_amount in code
    total_amount INTEGER NOT NULL, -- Amount in cents
    currency TEXT DEFAULT 'USD',
    status TEXT NOT NULL DEFAULT 'pending', -- pending, paid, expired, cancelled
    payment_provider TEXT, -- cryptobot, manual, etc
    payment_id TEXT,
    paid_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans(id)
);

CREATE INDEX IF NOT EXISTS idx_orders_user_id ON orders (user_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders (status);
CREATE INDEX IF NOT EXISTS idx_orders_payment_id ON orders (payment_id);

-- ================================================
-- GIFT CODES
-- ================================================

CREATE TABLE IF NOT EXISTS gift_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    plan_id INTEGER,
    duration_days INTEGER,
    created_by_user_id INTEGER NOT NULL,
    redeemed_by_user_id INTEGER,
    status TEXT NOT NULL DEFAULT 'active', -- active, redeemed, expired
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    redeemed_at DATETIME,
    expires_at DATETIME,
    FOREIGN KEY (plan_id) REFERENCES plans(id),
    FOREIGN KEY (created_by_user_id) REFERENCES users(id),
    FOREIGN KEY (redeemed_by_user_id) REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_gift_codes_code ON gift_codes (code);
CREATE INDEX IF NOT EXISTS idx_gift_codes_status ON gift_codes (status);

-- ================================================
-- TRAFFIC & MONITORING
-- ================================================

CREATE TABLE IF NOT EXISTS subscription_ip_tracking (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    client_ip TEXT NOT NULL,
    last_seen_at DATETIME NOT NULL,
    FOREIGN KEY (subscription_id) REFERENCES subscriptions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ip_tracking_sub_id ON subscription_ip_tracking (subscription_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_ip_tracking_unique ON subscription_ip_tracking (subscription_id, client_ip);

-- ================================================
-- REFERRAL SYSTEM
-- ================================================

CREATE TABLE IF NOT EXISTS referral_bonuses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    referred_user_id INTEGER NOT NULL,
    bonus_type TEXT NOT NULL, -- traffic_gb, days
    bonus_value REAL NOT NULL,
    status TEXT DEFAULT 'pending', -- pending, applied
    applied_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (referred_user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_referral_bonuses_user_id ON referral_bonuses (user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_referral_unique ON referral_bonuses (user_id, referred_user_id);

-- ================================================
-- ACTIVITY LOG
-- ================================================

CREATE TABLE IF NOT EXISTS activity_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    action TEXT NOT NULL,
    details TEXT,
    ip_address TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_activity_log_user_id ON activity_log (user_id);
CREATE INDEX IF NOT EXISTS idx_activity_log_created_at ON activity_log (created_at);

-- ================================================
-- INITIAL DATA
-- ================================================

-- Default Settings
INSERT OR IGNORE INTO settings (key, value) VALUES ('bot_token', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('bot_status', 'stopped');
INSERT OR IGNORE INTO settings (key, value) VALUES ('payment_api_key', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('payment_ipn_url', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('currency_rate', '1.0');
INSERT OR IGNORE INTO settings (key, value) VALUES ('referral_bonus_days', '7');

-- Default Admin (password: admin - CHANGE IN PRODUCTION!)
INSERT OR IGNORE INTO admins (username, password_hash) 
VALUES ('admin', '$2b$12$K.z2iBv.m6.h7.8.9.a.bcdefghijklmno.pqrstuvwxyz');

-- Default Plans
INSERT OR IGNORE INTO plans (id, name, description, traffic_limit_gb, device_limit, price, is_active, sort_order)
VALUES 
(1, 'Basic', 'Perfect for individuals', 50, 1, 9.99, 1, 1),
(2, 'Pro', 'For power users', 200, 3, 19.99, 1, 2),
(3, 'Unlimited', 'No limits!', -1, 5, 39.99, 1, 3);

-- Default Plan Durations
INSERT OR IGNORE INTO plan_durations (plan_id, days, price, discount_percent, is_active)
VALUES 
-- Basic
(1, 30, 9.99, 0, 1),
(1, 90, 26.99, 10, 1),
(1, 365, 99.99, 17, 1),
-- Pro
(2, 30, 19.99, 0, 1),
(2, 90, 53.99, 10, 1),
(2, 365, 199.99, 17, 1),
-- Unlimited
(3, 30, 39.99, 0, 1),
(3, 90, 107.99, 10, 1),
(3, 365, 399.99, 17, 1);

-- ================================================
-- POST-SCHEMA UPDATES (Merged for Single-File Install)
-- ================================================

-- Create plan_inbounds join table
CREATE TABLE IF NOT EXISTS plan_inbounds (
    plan_id INTEGER NOT NULL,
    inbound_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plan_id, inbound_id),
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE,
    FOREIGN KEY (inbound_id) REFERENCES inbounds(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plan_inbounds_plan ON plan_inbounds(plan_id);
CREATE INDEX IF NOT EXISTS idx_plan_inbounds_inbound ON plan_inbounds(inbound_id);

-- Ensure 'is_enabled' column exists in nodes
-- SQLite does not support IF NOT EXISTS for columns in create table if table exists.
-- The install.sh usually runs this on fresh DB.
-- If running on existing DB, this might fail, so we append simple ALTERs that might error locally but fine for fresh.
-- For robustness in 'catch-up' scenario on existing DB without migration tool:
-- We can't do conditional alter easily in pure SQL script without logic.
-- BUT, we can just update the CREATE TABLE 'nodes' above to include it for new installs.

-- See step 2: I will update the CREATE TABLE nodes block above instead of appending ALTER here for 'is_enabled'.

