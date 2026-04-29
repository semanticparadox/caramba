-- Бесплатный базовый план: ежедневное пополнение трафика
-- daily_traffic_mb: МБ, добавляемых подписчику каждые сутки (0 = не применяется)
-- is_free: план не требует оплаты, выдаётся автоматически при регистрации

ALTER TABLE plans ADD COLUMN IF NOT EXISTS daily_traffic_mb INTEGER DEFAULT 0;
ALTER TABLE plans ADD COLUMN IF NOT EXISTS is_free BOOLEAN DEFAULT FALSE;

-- Временная метка последнего суточного пополнения трафика по подписке
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS last_daily_topup_at TIMESTAMPTZ;
