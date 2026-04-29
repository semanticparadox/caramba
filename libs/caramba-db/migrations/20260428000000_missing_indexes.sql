-- Индексы на колонки, по которым часто выполняются запросы, но которые
-- отсутствовали в схеме. Добавляются без блокировки (CONCURRENTLY не работает
-- внутри транзакции миграции, поэтому используем IF NOT EXISTS).

-- subscriptions.expires_at — используется в авторенюэле и проверке истекших подписок
CREATE INDEX IF NOT EXISTS idx_subscriptions_expires_at
    ON subscriptions (expires_at);

-- subscriptions.plan_id — частое JOIN с таблицей plans
CREATE INDEX IF NOT EXISTS idx_subscriptions_plan_id
    ON subscriptions (plan_id);

-- payments.user_id — нет FK-индекса, а колонка часто участвует в WHERE и JOIN
CREATE INDEX IF NOT EXISTS idx_payments_user_id
    ON payments (user_id);

-- payment_sessions.user_id — аналогично
CREATE INDEX IF NOT EXISTS idx_payment_sessions_user_id
    ON payment_sessions (user_id);

-- payment_sessions.status — фильтр по статусу (pending/completed) в фоновых задачах
CREATE INDEX IF NOT EXISTS idx_payment_sessions_status
    ON payment_sessions (status);
