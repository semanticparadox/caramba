-- ============================================================
-- Система уведомлений и тикетов поддержки
-- ============================================================

-- Таблица уведомлений пользователей (inbox Mini App + бот)
CREATE TABLE IF NOT EXISTS user_notifications (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category TEXT NOT NULL,       -- payment | subscription | device | referral | support_ticket | system_maintenance
    severity TEXT NOT NULL DEFAULT 'info',  -- info | warning | error
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    payload_json JSONB,           -- {"ticket_id": 7, "url": "/tickets/7"}
    status TEXT NOT NULL DEFAULT 'unread',  -- unread | read | archived
    delivered_via_bot BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    read_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_user_notifications_user_unread
    ON user_notifications(user_id, created_at DESC) WHERE status = 'unread';

CREATE INDEX IF NOT EXISTS idx_user_notifications_user_all
    ON user_notifications(user_id, created_at DESC);

-- Расширенная таблица настроек уведомлений по каналам (заменяет blob-строку в notification_preferences).
-- Старая таблица notification_preferences (с колонками notify_new_device и т.д.) остаётся
-- нетронутой для обратной совместимости с /api/client/user/notifications.
CREATE TABLE IF NOT EXISTS notification_channel_prefs (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    channel TEXT NOT NULL,        -- bot_dm | mini_app
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (user_id, category, channel)
);

-- ============================================================
-- Тикеты поддержки
-- ============================================================

CREATE TABLE IF NOT EXISTS tickets (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category TEXT NOT NULL,       -- billing | connection | device | feature_request | other
    subject TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',  -- open | in_progress | awaiting_user | resolved | closed
    assignee_tg_id BIGINT,        -- tg_id администратора, взявшего тикет; NULL пока не назначен
    related_payment_id BIGINT REFERENCES payments(id) ON DELETE SET NULL,
    related_subscription_id BIGINT REFERENCES subscriptions(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_tickets_user ON tickets(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tickets_status_updated ON tickets(status, updated_at DESC);

-- Сообщения в тикете
CREATE TABLE IF NOT EXISTS ticket_messages (
    id BIGSERIAL PRIMARY KEY,
    ticket_id BIGINT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    sender_role TEXT NOT NULL,    -- user | admin | system
    sender_tg_id BIGINT,          -- NULL для системных сообщений
    body TEXT NOT NULL,
    attachments_json JSONB,       -- [{filename, mime, size_bytes, storage_key, url}]
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ticket_messages_ticket ON ticket_messages(ticket_id, created_at);

-- Файловые вложения тикетов (хранятся локально на сервере)
CREATE TABLE IF NOT EXISTS ticket_attachments (
    id BIGSERIAL PRIMARY KEY,
    ticket_id BIGINT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    message_id BIGINT REFERENCES ticket_messages(id) ON DELETE SET NULL,
    filename TEXT NOT NULL,
    mime_type TEXT,
    size_bytes BIGINT NOT NULL,
    storage_path TEXT NOT NULL,   -- путь под TICKETS_UPLOAD_DIR (по умолчанию /var/lib/caramba/tickets)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ticket_attachments_ticket ON ticket_attachments(ticket_id);
