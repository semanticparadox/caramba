-- ================================================
-- ПОДЕНЕВНАЯ ИСТОРИЯ ТРАФИКА ДЛЯ STANDALONE-ПРИЛОЖЕНИЯ
-- ------------------------------------------------
-- Flutter-клиент рисует график трафика (fl_chart) за ~30 дней. Существующие
-- источники не дают подневной разбивки по пользователю:
--   * subscriptions.used_traffic — только накопительный счётчик (cumulative);
--   * daily_stats.traffic_used   — системный агрегат (по всем юзерам, без up/down).
-- Поэтому заводим лёгкую таблицу подневных дельт на пользователя.
--
-- Узел рапортует ОДИН счётчик байт на пользователя (user_usage в node.rs), без
-- разделения на upload/download. Поэтому колонки up_bytes/down_bytes заведены
-- на будущее: пока весь объём пишется в down_bytes (доминирующее направление),
-- а up_bytes = 0. Когда агент начнёт отдавать раздельные счётчики, заполнение
-- разведётся без миграции схемы. Эндпоинт /api/v2/app/traffic отдаёт все три:
-- up_bytes, down_bytes и их сумму, так что контракт с клиентом не сломается.
-- ================================================

CREATE TABLE IF NOT EXISTS app_traffic_daily (
    id              BIGSERIAL PRIMARY KEY,
    user_id         BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- День в UTC (date_trunc на стороне записи).
    day             DATE NOT NULL,
    up_bytes        BIGINT NOT NULL DEFAULT 0,
    down_bytes      BIGINT NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Одна строка на (пользователь, день) — UPSERT накапливает дельты.
    UNIQUE(user_id, day)
);

-- Выборка истории идёт по user_id с сортировкой/фильтром по дню.
CREATE INDEX IF NOT EXISTS idx_app_traffic_daily_user_day
    ON app_traffic_daily (user_id, day DESC);
