-- Daily Analytics
CREATE TABLE IF NOT EXISTS daily_stats (
    date DATE PRIMARY KEY, -- YYYY-MM-DD
    new_users INTEGER DEFAULT 0,
    active_users INTEGER DEFAULT 0, -- DAU
    total_orders INTEGER DEFAULT 0,
    total_revenue INTEGER DEFAULT 0, -- in cents
    traffic_used INTEGER DEFAULT 0, -- in bytes
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Index for date range queries
CREATE INDEX IF NOT EXISTS idx_daily_stats_date ON daily_stats(date);

-- DAU Tracking (Unique Users per Day)
CREATE TABLE IF NOT EXISTS user_daily_activity (
    user_id INTEGER,
    date DATE,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, date)
);

