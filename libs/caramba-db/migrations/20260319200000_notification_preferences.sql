-- User notification preferences
CREATE TABLE IF NOT EXISTS notification_preferences (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    notify_new_device BOOLEAN NOT NULL DEFAULT TRUE,
    notify_traffic_warnings BOOLEAN NOT NULL DEFAULT TRUE,
    notify_expiry_reminders BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
