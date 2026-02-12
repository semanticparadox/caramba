-- ================================================
-- EXA ROBOT - Consolidated Database Schema
-- Version: 2026-02-05 (Unified Init)
-- ================================================
-- This single file contains ALL tables and features
-- for a fresh installation.
-- ================================================

-- ================================================
-- CORE TABLES
-- ================================================

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

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
    status TEXT NOT NULL DEFAULT 'new',
    root_password TEXT,
    vpn_port INTEGER NOT NULL DEFAULT 443,
    last_seen DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    
    -- Reality Keys
    reality_pub TEXT,
    reality_priv TEXT,
    short_id TEXT,
    domain TEXT,
    reality_sni TEXT DEFAULT 'www.google.com',
    
    -- Smart Setup
    join_token TEXT,
    auto_configure BOOLEAN DEFAULT 0,
    is_enabled BOOLEAN DEFAULT 1,
    certificates_status TEXT DEFAULT NULL,
    
    -- GeoIP (013_node_geoip)
    latitude REAL,
    longitude REAL,
    country_code TEXT,
    
    -- Telemetry (016_node_telemetry)
    last_latency FLOAT,
    last_cpu FLOAT,
    last_ram FLOAT,
    
    -- Bandwidth Shaping (015_bandwidth_shaping)
    config_qos_enabled BOOLEAN DEFAULT 0,
    config_block_torrent BOOLEAN DEFAULT 0,
    config_block_ads BOOLEAN DEFAULT 0,
    config_block_porn BOOLEAN DEFAULT 0,

    -- Display & Metadata
    country TEXT,
    city TEXT,
    flag TEXT,
    load_stats TEXT,
    check_stats_json TEXT,
    sort_order INTEGER DEFAULT 0,

    -- Limits & Load Balancing
    speed_limit_mbps INTEGER DEFAULT 0,
    max_users INTEGER DEFAULT 0,
    current_speed_mbps INTEGER DEFAULT 0,

    -- Phase 21: Advanced Telemetry & Connections
    active_connections INTEGER DEFAULT 0,
    total_ingress BIGINT DEFAULT 0,
    total_egress BIGINT DEFAULT 0,
    uptime BIGINT DEFAULT 0,
    last_session_ingress BIGINT DEFAULT 0,
    last_session_egress BIGINT DEFAULT 0
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
    protocol TEXT NOT NULL,
    listen_port INTEGER NOT NULL DEFAULT 443,
    listen_ip TEXT DEFAULT '::',
    settings TEXT NOT NULL DEFAULT '{}',
    stream_settings TEXT NOT NULL DEFAULT '{}',
    remark TEXT,
    enable BOOLEAN DEFAULT 1,
    last_rotated_at DATETIME,
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
    full_name TEXT,
    language_code TEXT DEFAULT 'en',
    is_banned BOOLEAN DEFAULT 0,
    ban_reason TEXT,
    banned_at DATETIME,
    referrer_id INTEGER,
    referral_code TEXT UNIQUE,
    referred_by INTEGER REFERENCES users(id),
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_seen DATETIME DEFAULT CURRENT_TIMESTAMP,
    
    -- Balance System (012_billing_referrals)
    balance INTEGER DEFAULT 0,
    
    -- Trial System (009_quick_wins)
    trial_used INTEGER DEFAULT 0,
    trial_used_at TIMESTAMP NULL,
    
    -- Channel Trials (013_channel_trials)
    channel_member_verified INTEGER DEFAULT 0,
    channel_verified_at TIMESTAMP,
    trial_source TEXT DEFAULT 'default',
    
    -- Terms & Warnings
    terms_accepted_at DATETIME,
    warning_count INTEGER DEFAULT 0,
    
    -- Bot History Tracking
    last_bot_msg_id INTEGER,
    
    -- Family Plans
    parent_id INTEGER DEFAULT NULL REFERENCES users(id),
    
    -- Enterprise Organizations (Phase 3)
    current_org_id INTEGER REFERENCES organizations(id) ON DELETE SET NULL,
    
    FOREIGN KEY (referrer_id) REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_users_parent_id ON users(parent_id);

-- ================================================
-- ENTERPRISE ORGANIZATIONS
-- ================================================

CREATE TABLE IF NOT EXISTS organizations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT UNIQUE,
    balance INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS organization_members (
    organization_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    role TEXT NOT NULL DEFAULT 'member', -- 'owner', 'admin', 'member'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (organization_id, user_id),
    FOREIGN KEY (organization_id) REFERENCES organizations(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_users_tg_id ON users (tg_id);
CREATE INDEX IF NOT EXISTS idx_users_referral_code ON users (referral_code);
CREATE INDEX IF NOT EXISTS idx_users_referrer_id ON users (referrer_id);
CREATE INDEX IF NOT EXISTS idx_users_referred_by ON users(referred_by);
CREATE INDEX IF NOT EXISTS idx_users_channel_verified ON users(channel_member_verified);
CREATE INDEX IF NOT EXISTS idx_users_trial_source ON users(trial_source);

-- ================================================
-- BOT CHAT HISTORY
-- ================================================

CREATE TABLE IF NOT EXISTS bot_chat_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL references users(id) ON DELETE CASCADE,
    chat_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_history_user_created ON bot_chat_history(user_id, created_at);

-- ================================================
-- STORE & SUBSCRIPTIONS
-- ================================================

CREATE TABLE IF NOT EXISTS plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    traffic_limit_gb INTEGER NOT NULL DEFAULT 0,
    device_limit INTEGER DEFAULT 1,
    price INTEGER NOT NULL,
    is_active BOOLEAN DEFAULT 1,
    is_trial INTEGER DEFAULT 0,
    sort_order INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS plan_durations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id INTEGER NOT NULL,
    duration_days INTEGER NOT NULL,
    traffic_gb INTEGER,
    price INTEGER NOT NULL,
    discount_percent REAL DEFAULT 0,
    is_active BOOLEAN DEFAULT 1,
    is_default INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plan_durations_plan_id ON plan_durations (plan_id);

-- Plan-Node relationships (007_plan_nodes)
CREATE TABLE IF NOT EXISTS plan_nodes (
    plan_id INTEGER NOT NULL,
    node_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plan_id, node_id),
    FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE,
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

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

CREATE TABLE IF NOT EXISTS subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    plan_id INTEGER NOT NULL,
    node_id INTEGER,
    vless_uuid TEXT UNIQUE,
    subscription_uuid TEXT UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending',
    used_traffic INTEGER DEFAULT 0,
    device_count INTEGER DEFAULT 0,
    activated_at DATETIME,
    expires_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    traffic_updated_at DATETIME,
    note TEXT,
    
    -- Auto-Renewal (009_quick_wins)
    auto_renew INTEGER DEFAULT 0,
    alerts_sent TEXT DEFAULT '[]',
    is_trial INTEGER DEFAULT 0,
    
    -- Subscription URLs (010_subscription_urls)
    last_sub_access TIMESTAMP NULL,
    last_access_ip TEXT,
    last_access_ua TEXT,
    
    -- Enterprise Organizations
    organization_id INTEGER,
    
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans(id),
    FOREIGN KEY (organization_id) REFERENCES organizations(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user_id ON subscriptions (user_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions (status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_vless_uuid ON subscriptions (vless_uuid);
CREATE INDEX IF NOT EXISTS idx_subscriptions_uuid ON subscriptions(subscription_uuid);
CREATE INDEX IF NOT EXISTS idx_subscriptions_auto_renew ON subscriptions(expires_at, auto_renew, status);

-- ================================================
-- PRODUCTS & CART (014_cart_items)
-- ================================================

CREATE TABLE IF NOT EXISTS products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id INTEGER,
    name TEXT NOT NULL,
    description TEXT,
    price INTEGER NOT NULL,
    product_type TEXT NOT NULL DEFAULT 'subscription',
    content TEXT,
    is_active BOOLEAN DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS cart_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    product_id INTEGER NOT NULL,
    quantity INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY(product_id) REFERENCES products(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cart_items_user ON cart_items(user_id);

-- ================================================
-- PAYMENTS & ORDERS (012_billing_referrals)
-- ================================================

CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    plan_id INTEGER NOT NULL,
    duration_days INTEGER NOT NULL,
    amount REAL NOT NULL,
    total_amount INTEGER NOT NULL,
    currency TEXT DEFAULT 'USD',
    status TEXT NOT NULL DEFAULT 'pending',
    payment_provider TEXT,
    payment_id TEXT,
    paid_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES plans(id)
);

CREATE INDEX IF NOT EXISTS idx_orders_user_id ON orders (user_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders (status);
CREATE INDEX IF NOT EXISTS idx_orders_payment_id ON orders (payment_id);

CREATE TABLE IF NOT EXISTS payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    amount INTEGER NOT NULL,
    method TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    transaction_id TEXT,
    FOREIGN KEY(user_id) REFERENCES users(id)
);

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
    status TEXT NOT NULL DEFAULT 'active',
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
    user_agent TEXT,
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
    bonus_type TEXT NOT NULL,
    bonus_value REAL NOT NULL,
    status TEXT DEFAULT 'pending',
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
-- SNI ROTATION (002_sni_rotation)
-- ================================================

CREATE TABLE IF NOT EXISTS sni_pool (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL UNIQUE,
    tier INTEGER DEFAULT 0,
    health_score INTEGER DEFAULT 100,
    last_check TIMESTAMP,
    is_active BOOLEAN DEFAULT 1,
    notes TEXT
);

CREATE TABLE IF NOT EXISTS sni_rotation_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL,
    old_sni TEXT NOT NULL,
    new_sni TEXT NOT NULL,
    reason TEXT,
    rotated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (node_id) REFERENCES nodes(id)
);

-- ================================================
-- ANALYTICS (008_analytics_daily)
-- ================================================

CREATE TABLE IF NOT EXISTS daily_stats (
    date DATE PRIMARY KEY,
    new_users INTEGER DEFAULT 0,
    active_users INTEGER DEFAULT 0,
    total_orders INTEGER DEFAULT 0,
    total_revenue INTEGER DEFAULT 0,
    traffic_used INTEGER DEFAULT 0,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_daily_stats_date ON daily_stats(date);

CREATE TABLE IF NOT EXISTS user_daily_activity (
    user_id INTEGER,
    date DATE,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, date)
);

-- ================================================
-- FRONTEND SERVERS (011_frontend_servers + 012_secure_frontend_tokens)
-- ================================================

CREATE TABLE IF NOT EXISTS frontend_servers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL UNIQUE,
    ip_address TEXT NOT NULL,
    region TEXT NOT NULL,
    miniapp_domain TEXT,
    sub_path TEXT DEFAULT '/sub/',
    auth_token TEXT,
    auth_token_hash TEXT,
    token_expires_at TIMESTAMP,
    token_rotated_at TIMESTAMP,
    is_active INTEGER DEFAULT 1,
    status TEXT DEFAULT 'offline',
    last_heartbeat TIMESTAMP,
    traffic_monthly INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS frontend_server_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    frontend_id INTEGER NOT NULL,
    requests_count INTEGER DEFAULT 0,
    bandwidth_used INTEGER DEFAULT 0,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (frontend_id) REFERENCES frontend_servers(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_frontend_domain ON frontend_servers(domain);
CREATE INDEX IF NOT EXISTS idx_frontend_region ON frontend_servers(region);
CREATE INDEX IF NOT EXISTS idx_frontend_active ON frontend_servers(is_active);
CREATE INDEX IF NOT EXISTS idx_frontend_status ON frontend_servers(status);
CREATE INDEX IF NOT EXISTS idx_frontend_stats_time ON frontend_server_stats(timestamp);
CREATE INDEX IF NOT EXISTS idx_frontend_stats_server ON frontend_server_stats(frontend_id);
CREATE INDEX IF NOT EXISTS idx_frontend_token_expiration ON frontend_servers(token_expires_at);
CREATE INDEX IF NOT EXISTS idx_frontend_auth_hash ON frontend_servers(auth_token_hash);

-- ================================================
-- API KEYS (Enrollment) (018_api_keys)
-- ================================================

CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'enrollment', -- 'enrollment', 'admin', etc.
    max_uses INTEGER, -- NULL = unlimited
    current_uses INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT 1,
    expires_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    created_by INTEGER, -- User ID who created it
    FOREIGN KEY (created_by) REFERENCES admins(id)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_key ON api_keys(key);

-- ================================================
-- NODE GROUPS & TEMPLATES (Phase 1.8)
-- ================================================

CREATE TABLE IF NOT EXISTS node_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT UNIQUE,
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS node_group_members (
    node_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(node_id, group_id),
    FOREIGN KEY(node_id) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY(group_id) REFERENCES node_groups(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS plan_groups (
    plan_id INTEGER NOT NULL,
    group_id INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(plan_id, group_id),
    FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE,
    FOREIGN KEY(group_id) REFERENCES node_groups(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS inbound_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL,
    settings_template TEXT NOT NULL,
    stream_settings_template TEXT NOT NULL,
    target_group_id INTEGER,
    port_range_start INTEGER DEFAULT 10000,
    port_range_end INTEGER DEFAULT 60000,
    renew_interval_hours INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(target_group_id) REFERENCES node_groups(id) ON DELETE SET NULL
);

-- ================================================
-- FAMILY PLANS
-- ================================================

CREATE TABLE IF NOT EXISTS family_invites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    parent_id INTEGER NOT NULL REFERENCES users(id),
    max_uses INTEGER NOT NULL DEFAULT 1,
    used_count INTEGER NOT NULL DEFAULT 0,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_family_invites_code ON family_invites(code);
CREATE INDEX IF NOT EXISTS idx_family_invites_parent_id ON family_invites(parent_id);

-- ================================================
-- INITIAL DATA
-- ================================================

INSERT OR IGNORE INTO settings (key, value) VALUES ('bot_token', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('bot_status', 'stopped');
INSERT OR IGNORE INTO settings (key, value) VALUES ('payment_api_key', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('payment_ipn_url', '');
INSERT OR IGNORE INTO settings (key, value) VALUES ('currency_rate', '1.0');
INSERT OR IGNORE INTO settings (key, value) VALUES ('referral_bonus_days', '7');

-- Default Admin (password: admin - CHANGE IN PRODUCTION!)
INSERT OR IGNORE INTO admins (username, password_hash) 
VALUES ('admin', '$2b$12$K.z2iBv.m6.h7.8.9.a.bcdefghijklmno.pqrstuvwxyz');

-- Seed SNI Pool
-- Russian Legacy & Essential Services
INSERT OR IGNORE INTO sni_pool (domain, tier, notes) VALUES
    ('gosuslugi.ru', 0, 'Public Services'),
    ('nalog.gov.ru', 0, 'Tax Service'),
    ('sberbank.ru', 0, 'Banking'),
    ('online.sberbank.ru', 0, 'Banking'),
    ('tinkoff.ru', 0, 'Banking'),
    ('vtb.ru', 0, 'Banking'),
    ('yandex.ru', 0, 'Search/Portal'),
    ('ya.ru', 0, 'Search/Portal'),
    ('yandex.net', 0, 'Yandex Infrastructure'),
    ('yastatic.net', 0, 'Yandex CDN'),
    ('vk.com', 0, 'Social Network'),
    ('vk-cdn.net', 0, 'VK CDN'),
    ('mail.ru', 0, 'Email/Portal'),
    ('ok.ru', 0, 'Social Network'),
    ('wildberries.ru', 0, 'E-commerce'),
    ('ozon.ru', 0, 'E-commerce'),
    ('avito.ru', 0, 'Classifieds'),
    ('rt.ru', 0, 'Rostelecom'),
    ('rbc.ru', 0, 'News');

-- Regional / CIS Variations
INSERT OR IGNORE INTO sni_pool (domain, tier, notes) VALUES
    ('yandex.kz', 1, 'Yandex Kazakhstan'),
    ('ozon.kz', 1, 'Ozon Kazakhstan'),
    ('kaspi.kz', 1, 'Kaspi Kazakhstan'),
    ('kolesa.kz', 1, 'Classifieds Kazakhstan'),
    ('olx.kz', 1, 'Classifieds Kazakhstan'),
    ('yandex.by', 1, 'Yandex Belarus'),
    ('onliner.by', 1, 'Portal Belarus');

-- System & Hardware Updates (High Trust)
INSERT OR IGNORE INTO sni_pool (domain, tier, notes) VALUES
    ('swscan.apple.com', 0, 'Apple Software Update'),
    ('swcdn.apple.com', 0, 'Apple software distribution'),
    ('updates.cdn-apple.com', 0, 'Apple update CDN'),
    ('windowsupdate.microsoft.com', 0, 'Windows Update'),
    ('download.microsoft.com', 0, 'Microsoft Downloads'),
    ('office.com', 0, 'Microsoft Office'),
    ('update.googleapis.com', 0, 'Android/Google Updates'),
    ('dl.google.com', 0, 'Google Downloads'),
    ('samsung.com', 0, 'Samsung'),
    ('samsungcloud.com', 0, 'Samsung Cloud'),
    ('xiaomi.com', 0, 'Xiaomi'),
    ('miui.com', 0, 'Xiaomi MIUI'),
    ('play.google.com', 0, 'Google Play Store');

-- Global CDN & Infrastructure
INSERT OR IGNORE INTO sni_pool (domain, tier, notes) VALUES
    ('cdnjs.cloudflare.com', 1, 'Cloudflare CDN'),
    ('ajax.googleapis.com', 1, 'Google APIs'),
    ('fonts.googleapis.com', 1, 'Google Fonts'),
    ('cdn.jsdelivr.net', 1, 'jsDelivr CDN'),
    ('static.doubleclick.net', 2, 'Google Ads Static'),
    ('github.com', 1, 'GitHub'),
    ('bitbucket.org', 1, 'Bitbucket'),
    ('gitlab.com', 1, 'GitLab'),
    ('docker.com', 1, 'Docker'),
    ('visualstudio.com', 1, 'Visual Studio');

-- Media & Entertainment (Low Tier fallbacks)
INSERT OR IGNORE INTO sni_pool (domain, tier, notes) VALUES
    ('steampowered.com', 2, 'Steam'),
    ('steamcommunity.com', 2, 'Steam Community'),
    ('epicgames.com', 2, 'Epic Games'),
    ('playstation.com', 2, 'PlayStation'),
    ('xbox.com', 2, 'Xbox');

-- Create Free Trial Plan (Fixed INSERT with traffic_limit_gb)
INSERT OR IGNORE INTO plans (name, description, traffic_limit_gb, price, is_active, is_trial)
VALUES ('Free Trial', '24h trial with 10GB traffic', 10, 0, 1, 1);

-- Add trial duration (only if plan exists)
INSERT OR IGNORE INTO plan_durations (plan_id, duration_days, traffic_gb, price, is_default)
SELECT id, 1, 10, 0, 1
FROM plans WHERE is_trial = 1;

-- Seed default group
INSERT OR IGNORE INTO node_groups (id, name, slug, description) VALUES (1, 'Default', 'default', 'Default Node Group');

-- ================================================
-- CLEANUP / SAFETY
-- ================================================

-- Disable AmneziaWG by default (Safety catch for fresh installs if default value was 1)
UPDATE inbounds SET enable = 0 WHERE protocol = 'amneziawg';
