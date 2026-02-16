-- Re-create promo_codes table to ensure correct schema (Fixing crash)
DROP TABLE IF EXISTS promo_codes;
CREATE TABLE promo_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL, -- 'subscription', 'balance', 'trial_traffic'
    plan_id INTEGER, -- For 'subscription'
    balance_amount INTEGER, -- For 'balance'
    duration_days INTEGER,
    traffic_gb INTEGER,
    max_uses INTEGER DEFAULT 1,
    current_uses INTEGER DEFAULT 0,
    expires_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    created_by_admin_id INTEGER, -- Created by panel admin
    promoter_user_id INTEGER, -- Link to a user if this is a promoter's code
    is_active BOOLEAN DEFAULT 1,
    FOREIGN KEY (plan_id) REFERENCES plans(id)
);

CREATE TABLE IF NOT EXISTS promo_code_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    promo_code_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    used_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (promo_code_id) REFERENCES promo_codes(id),
    FOREIGN KEY (user_id) REFERENCES users(id),
    UNIQUE(promo_code_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_promo_codes_code ON promo_codes (code);
CREATE INDEX IF NOT EXISTS idx_promo_code_usage_user ON promo_code_usage (user_id);
