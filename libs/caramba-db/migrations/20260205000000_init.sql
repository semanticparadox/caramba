-- ================================================
-- CARAMBA - Consolidated Database Schema (PostgreSQL)
-- Version: 2026-02-17 (Postgres Migration)
-- ================================================

-- ================================================
-- CORE TABLES
-- ================================================

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS admins (
    id BIGSERIAL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- NODE MANAGEMENT
-- ================================================

CREATE TABLE IF NOT EXISTS nodes (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    ip TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'new',
    root_password TEXT,
    vpn_port INTEGER NOT NULL DEFAULT 443,
    last_seen TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    
    reality_pub TEXT,
    reality_priv TEXT,
    short_id TEXT,
    domain TEXT,
    reality_sni TEXT DEFAULT 'www.google.com',
    
    join_token TEXT,
    auto_configure BOOLEAN DEFAULT FALSE,
    is_enabled BOOLEAN DEFAULT TRUE,
    certificates_status TEXT DEFAULT NULL,
    
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    country_code TEXT,
    
    last_latency DOUBLE PRECISION,
    last_cpu DOUBLE PRECISION,
    last_ram DOUBLE PRECISION,
    
    config_qos_enabled BOOLEAN DEFAULT FALSE,
    config_block_torrent BOOLEAN DEFAULT FALSE,
    config_block_ads BOOLEAN DEFAULT FALSE,
    config_block_porn BOOLEAN DEFAULT FALSE,

    country TEXT,
    city TEXT,
    flag TEXT,
    load_stats TEXT,
    check_stats_json TEXT,
    sort_order INTEGER DEFAULT 0,

    speed_limit_mbps INTEGER DEFAULT 0,
    max_users INTEGER DEFAULT 0,
    current_speed_mbps INTEGER DEFAULT 0,

    active_connections INTEGER DEFAULT 0,
    total_ingress BIGINT DEFAULT 0,
    total_egress BIGINT DEFAULT 0,
    uptime BIGINT DEFAULT 0,
    last_session_ingress BIGINT DEFAULT 0,
    last_session_egress BIGINT DEFAULT 0,
    doomsday_password TEXT,
    
    version TEXT,
    is_relay BOOLEAN NOT NULL DEFAULT FALSE,
    last_sync_trigger TEXT,
    pending_log_collection BOOLEAN NOT NULL DEFAULT FALSE,
    
    max_ram BIGINT DEFAULT 0,
    cpu_cores INTEGER DEFAULT 0,
    cpu_model TEXT
);

CREATE INDEX IF NOT EXISTS idx_nodes_ip ON nodes (ip);
CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes (status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_nodes_join_token ON nodes (join_token);

-- ================================================
-- NETWORK CONFIGURATION
-- ================================================

CREATE TABLE IF NOT EXISTS inbounds (
    id BIGSERIAL PRIMARY KEY,
    node_id BIGINT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    protocol TEXT NOT NULL,
    listen_port INTEGER NOT NULL DEFAULT 443,
    listen_ip TEXT DEFAULT '::',
    settings TEXT NOT NULL DEFAULT '{}',
    stream_settings TEXT NOT NULL DEFAULT '{}',
    remark TEXT,
    enable BOOLEAN DEFAULT TRUE,
    renew_interval_mins INTEGER DEFAULT 0,
    port_range_start INTEGER DEFAULT 10000,
    port_range_end INTEGER DEFAULT 60000,
    last_rotated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(node_id, listen_port)
);

CREATE INDEX IF NOT EXISTS idx_inbounds_node ON inbounds (node_id);
CREATE INDEX IF NOT EXISTS idx_inbounds_protocol ON inbounds (protocol);

-- ================================================
-- USER MANAGEMENT
-- ================================================

CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    tg_id BIGINT NOT NULL UNIQUE,
    username TEXT,
    first_name TEXT,
    last_name TEXT,
    full_name TEXT,
    language_code TEXT DEFAULT 'en',
    is_banned BOOLEAN DEFAULT FALSE,
    ban_reason TEXT,
    banned_at TIMESTAMPTZ,
    referrer_id BIGINT REFERENCES users(id),
    referral_code TEXT UNIQUE,
    referred_by BIGINT REFERENCES users(id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    last_seen TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    
    balance INTEGER DEFAULT 0,
    trial_used BOOLEAN DEFAULT FALSE,
    trial_used_at TIMESTAMPTZ NULL,
    
    channel_member_verified BOOLEAN DEFAULT FALSE,
    channel_verified_at TIMESTAMPTZ,
    trial_source TEXT DEFAULT 'default',
    
    terms_accepted_at TIMESTAMPTZ,
    warning_count INTEGER DEFAULT 0,
    last_bot_msg_id INTEGER,
    parent_id BIGINT DEFAULT NULL REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_users_parent_id ON users(parent_id);

-- ================================================
-- ENTERPRISE ORGANIZATIONS
-- ================================================

CREATE TABLE IF NOT EXISTS organizations (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT UNIQUE,
    balance INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS organization_members (
    organization_id BIGINT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'member', 
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (organization_id, user_id)
);

-- ================================================
-- BOT CHAT HISTORY
-- ================================================

CREATE TABLE IF NOT EXISTS bot_chat_history (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chat_id BIGINT NOT NULL,
    message_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_history_user_created ON bot_chat_history(user_id, created_at);

-- ================================================
-- STORE & SUBSCRIPTIONS
-- ================================================

CREATE TABLE IF NOT EXISTS plans (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    traffic_limit_gb INTEGER NOT NULL DEFAULT 0,
    device_limit INTEGER DEFAULT 1,
    price INTEGER NOT NULL,
    is_active BOOLEAN DEFAULT TRUE,
    is_trial BOOLEAN DEFAULT FALSE,
    sort_order INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS plan_durations (
    id BIGSERIAL PRIMARY KEY,
    plan_id BIGINT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    duration_days INTEGER NOT NULL,
    traffic_gb INTEGER,
    price INTEGER NOT NULL,
    discount_percent REAL DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE,
    is_default BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_plan_durations_plan_id ON plan_durations (plan_id);

CREATE TABLE IF NOT EXISTS plan_nodes (
    plan_id BIGINT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    node_id BIGINT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plan_id, node_id)
);

CREATE TABLE IF NOT EXISTS plan_inbounds (
    plan_id BIGINT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    inbound_id BIGINT NOT NULL REFERENCES inbounds(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plan_id, inbound_id)
);

CREATE INDEX IF NOT EXISTS idx_plan_inbounds_plan ON plan_inbounds(plan_id);
CREATE INDEX IF NOT EXISTS idx_plan_inbounds_inbound ON plan_inbounds(inbound_id);

CREATE TABLE IF NOT EXISTS subscriptions (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plan_id BIGINT NOT NULL REFERENCES plans(id),
    node_id BIGINT REFERENCES nodes(id),
    vless_uuid TEXT UNIQUE,
    subscription_uuid TEXT UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending',
    used_traffic BIGINT DEFAULT 0,
    device_count INTEGER DEFAULT 0,
    activated_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    traffic_updated_at TIMESTAMPTZ,
    note TEXT,
    auto_renew BOOLEAN DEFAULT FALSE,
    alerts_sent TEXT DEFAULT '[]',
    is_trial BOOLEAN DEFAULT FALSE,
    last_sub_access TIMESTAMPTZ NULL,
    last_access_ip TEXT,
    last_access_ua TEXT,
    organization_id BIGINT REFERENCES organizations(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user_id ON subscriptions (user_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions (status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_vless_uuid ON subscriptions (vless_uuid);
CREATE INDEX IF NOT EXISTS idx_subscriptions_uuid ON subscriptions(subscription_uuid);

-- ================================================
-- PRODUCTS & CART
-- ================================================

CREATE TABLE IF NOT EXISTS products (
    id BIGSERIAL PRIMARY KEY,
    category_id BIGINT,
    name TEXT NOT NULL,
    description TEXT,
    price INTEGER NOT NULL,
    product_type TEXT NOT NULL DEFAULT 'subscription',
    content TEXT,
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS cart_items (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    quantity INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- PAYMENTS & ORDERS
-- ================================================

CREATE TABLE IF NOT EXISTS orders (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plan_id BIGINT NOT NULL REFERENCES plans(id),
    duration_days INTEGER NOT NULL,
    amount DOUBLE PRECISION NOT NULL,
    total_amount INTEGER NOT NULL,
    currency TEXT DEFAULT 'USD',
    status TEXT NOT NULL DEFAULT 'pending',
    payment_provider TEXT,
    payment_id TEXT,
    paid_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS payments (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    amount INTEGER NOT NULL,
    method TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    transaction_id TEXT
);

-- ================================================
-- GIFT CODES
-- ================================================

CREATE TABLE IF NOT EXISTS gift_codes (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    plan_id BIGINT REFERENCES plans(id),
    duration_days INTEGER,
    created_by_user_id BIGINT NOT NULL REFERENCES users(id),
    redeemed_by_user_id BIGINT REFERENCES users(id),
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    redeemed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
);

-- ================================================
-- PROMO CODES
-- ================================================

CREATE TABLE IF NOT EXISTS promo_codes (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL, 
    plan_id BIGINT REFERENCES plans(id),
    balance_amount INTEGER, 
    duration_days INTEGER,
    traffic_gb INTEGER,
    max_uses INTEGER DEFAULT 1,
    current_uses INTEGER DEFAULT 0,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    created_by_admin_id BIGINT REFERENCES admins(id), 
    promoter_user_id BIGINT REFERENCES users(id), 
    is_active BOOLEAN DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS promo_code_usage (
    id BIGSERIAL PRIMARY KEY,
    promo_code_id BIGINT NOT NULL REFERENCES promo_codes(id),
    user_id BIGINT NOT NULL REFERENCES users(id),
    used_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(promo_code_id, user_id)
);

-- ================================================
-- TRAFFIC & MONITORING
-- ================================================

CREATE TABLE IF NOT EXISTS subscription_ip_tracking (
    id BIGSERIAL PRIMARY KEY,
    subscription_id BIGINT NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    client_ip TEXT NOT NULL,
    user_agent TEXT,
    last_seen_at TIMESTAMPTZ NOT NULL,
    UNIQUE(subscription_id, client_ip)
);

-- ================================================
-- REFERRAL SYSTEM
-- ================================================

CREATE TABLE IF NOT EXISTS referral_bonuses (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    referred_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    bonus_type TEXT NOT NULL,
    bonus_value DOUBLE PRECISION NOT NULL,
    status TEXT DEFAULT 'pending',
    applied_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, referred_user_id)
);

-- ================================================
-- ACTIVITY LOG
-- ================================================

CREATE TABLE IF NOT EXISTS activity_log (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    details TEXT,
    ip_address TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- SNI ROTATION
-- ================================================

CREATE TABLE IF NOT EXISTS sni_pool (
    id BIGSERIAL PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    tier INTEGER DEFAULT 0,
    health_score INTEGER DEFAULT 100,
    last_check TIMESTAMPTZ,
    is_active BOOLEAN DEFAULT TRUE,
    is_premium BOOLEAN NOT NULL DEFAULT FALSE,
    notes TEXT,
    discovered_by_node_id BIGINT REFERENCES nodes(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS node_pinned_snis (
    node_id BIGINT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    sni_id BIGINT NOT NULL REFERENCES sni_pool(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (node_id, sni_id)
);

CREATE TABLE IF NOT EXISTS sni_blacklist (
    domain TEXT PRIMARY KEY,
    reason TEXT,
    blocked_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sni_rotation_log (
    id BIGSERIAL PRIMARY KEY,
    node_id BIGINT NOT NULL REFERENCES nodes(id),
    old_sni TEXT NOT NULL,
    new_sni TEXT NOT NULL,
    reason TEXT,
    rotated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- ANALYTICS
-- ================================================

CREATE TABLE IF NOT EXISTS daily_stats (
    date DATE PRIMARY KEY,
    new_users INTEGER DEFAULT 0,
    active_users INTEGER DEFAULT 0,
    total_orders INTEGER DEFAULT 0,
    total_revenue INTEGER DEFAULT 0,
    traffic_used BIGINT DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS user_daily_activity (
    user_id BIGINT,
    date DATE,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, date)
);

-- ================================================
-- FRONTEND SERVERS
-- ================================================

CREATE TABLE IF NOT EXISTS frontend_servers (
    id BIGSERIAL PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    ip_address TEXT NOT NULL,
    region TEXT NOT NULL,
    miniapp_domain TEXT,
    sub_path TEXT DEFAULT '/sub/',
    auth_token TEXT,
    auth_token_hash TEXT,
    token_expires_at TIMESTAMPTZ,
    token_rotated_at TIMESTAMPTZ,
    is_active BOOLEAN DEFAULT TRUE,
    status TEXT DEFAULT 'offline',
    last_heartbeat TIMESTAMPTZ,
    traffic_monthly BIGINT DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS frontend_server_stats (
    id BIGSERIAL PRIMARY KEY,
    frontend_id BIGINT NOT NULL REFERENCES frontend_servers(id) ON DELETE CASCADE,
    requests_count BIGINT DEFAULT 0,
    bandwidth_used BIGINT DEFAULT 0,
    timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- API KEYS
-- ================================================

CREATE TABLE IF NOT EXISTS api_keys (
    id BIGSERIAL PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'enrollment',
    max_uses INTEGER, 
    current_uses INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT REFERENCES admins(id)
);

-- ================================================
-- NODE GROUPS & TEMPLATES
-- ================================================

CREATE TABLE IF NOT EXISTS node_groups (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS node_group_members (
    node_id BIGINT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    group_id BIGINT NOT NULL REFERENCES node_groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(node_id, group_id)
);

CREATE TABLE IF NOT EXISTS plan_groups (
    plan_id BIGINT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    group_id BIGINT NOT NULL REFERENCES node_groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(plan_id, group_id)
);

CREATE TABLE IF NOT EXISTS inbound_templates (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL,
    settings_template TEXT NOT NULL,
    stream_settings_template TEXT NOT NULL,
    target_group_id BIGINT REFERENCES node_groups(id) ON DELETE SET NULL,
    port_range_start INTEGER DEFAULT 10000,
    port_range_end INTEGER DEFAULT 60000,
    renew_interval_hours INTEGER DEFAULT 0,
    renew_interval_mins INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- FAMILY PLANS
-- ================================================

CREATE TABLE IF NOT EXISTS family_invites (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    parent_id BIGINT NOT NULL REFERENCES users(id),
    max_uses INTEGER NOT NULL DEFAULT 1,
    used_count INTEGER NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ================================================
-- INITIAL DATA
-- ================================================

INSERT INTO settings (key, value) VALUES ('bot_token', '') ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('bot_status', 'stopped') ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('payment_api_key', '') ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('payment_ipn_url', '') ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('currency_rate', '1.0') ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('referral_bonus_days', '7') ON CONFLICT (key) DO NOTHING;

-- Default Admin (password: admin)
INSERT INTO admins (username, password_hash) 
VALUES ('admin', '$2b$12$K.z2iBv.m6.h7.8.9.a.bcdefghijklmno.pqrstuvwxyz') 
ON CONFLICT (username) DO NOTHING;

-- Seed SNI Pool (Sample)
INSERT INTO sni_pool (domain, tier, notes) VALUES ('gosuslugi.ru', 0, 'Public Services') ON CONFLICT (domain) DO NOTHING;
INSERT INTO node_groups (id, name, slug, description) VALUES (1, 'Default', 'default', 'Default Node Group') ON CONFLICT (id) DO NOTHING;
