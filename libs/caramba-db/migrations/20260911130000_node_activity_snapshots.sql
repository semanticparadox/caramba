-- ================================================
-- СНАПШОТЫ ТРАФИКА УЗЛОВ И РУЧНОЙ ЛИМИТ ЁМКОСТИ
-- ------------------------------------------------
-- Зачем таблица снапшотов: в `nodes.total_ingress/total_egress` лежат ТОЛЬКО
-- накопительные счётчики, из них нельзя получить «сколько узел прокачал за
-- сутки». Прежняя разбивка трафика по узлам считалась из
-- `subscriptions.used_traffic` по `subscriptions.node_id`, а этот столбец
-- бывает NULL и указывает максимум на один узел из нескольких, реально
-- обслуживающих план, — то есть для мультинодовых планов она была структурно
-- неверна. Снапшот раз в 10 минут даёт честные окна 24 ч / 30 дней как разницу
-- соседних замеров и ни от чего, кроме самих счётчиков узла, не зависит.
--
-- Переустановка узла обнуляет счётчики: разница соседних точек уходит в минус.
-- Поэтому суммируются только ПОЛОЖИТЕЛЬНЫЕ шаги (GREATEST(diff, 0) в запросе
-- панели), а не «последний минус первый» — иначе один рестарт съедал бы всю
-- историю окна.
-- ================================================

CREATE TABLE IF NOT EXISTS node_traffic_snapshots (
    id            BIGSERIAL   PRIMARY KEY,
    node_id       BIGINT      NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    ts            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Копия накопительных счётчиков узла на момент замера, байты.
    total_ingress BIGINT      NOT NULL DEFAULT 0,
    total_egress  BIGINT      NOT NULL DEFAULT 0
);

-- Все выборки идут «по узлу за окно времени», и ретеншен режет по ts.
CREATE INDEX IF NOT EXISTS idx_node_traffic_snapshots_node_ts
    ON node_traffic_snapshots (node_id, ts DESC);
CREATE INDEX IF NOT EXISTS idx_node_traffic_snapshots_ts
    ON node_traffic_snapshots (ts);

-- Ручной потолок пользователей на узел. NULL означает «лимита руками не
-- задавали» — панель показывает расчётный `nodes.max_users`
-- (telemetry_service::derive_recommended_max_users). Отдельная колонка нужна
-- именно потому, что расчётное значение перезаписывается каждым heartbeat:
-- вписать своё число прямо в `max_users` нельзя, его затрёт телеметрия.
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS max_users_override INTEGER;

-- Индексы под выборки онлайна по узлу (W3 завёл таблицу под запись, читать её
-- по узлу с окном по времени начинает эта волна).
CREATE INDEX IF NOT EXISTS idx_node_user_activity_node_seen
    ON node_user_activity (node_id, last_seen_at DESC);
-- Обратный путь «где этот пользователь сейчас» для списка Users и карточки.
CREATE INDEX IF NOT EXISTS idx_node_user_activity_tag_seen
    ON node_user_activity (user_tag, last_seen_at DESC);
