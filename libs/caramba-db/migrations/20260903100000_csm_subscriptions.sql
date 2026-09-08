-- CSM/1: локатор подписки и счётчик версий директив.
--
-- Спецификация: apps/caramba-client/docs/protocol/03-WIRE.md раздел 4 (loc),
-- 02-SPEC.md 4.7 (кто выдаёт ver), 01-DECISION.md раздел 7 блок P3.
--
-- Почему это отдельная таблица, а не три колонки в subscriptions:
--   * локатор это HMAC от uuid подписки и поколения, его нельзя обратить, а
--     значит маршрут /sub/m1/{loc} ищет подписку ТОЛЬКО по индексной колонке;
--   * поколение живёт на подписке, а не на панели: одна утечка чинится одним
--     UPDATE (01-DECISION.md 5.5.3), и таблица с уникальным локатором делает
--     смену поколения атомарной;
--   * счётчик директив общий для всех устройств подписки (02-SPEC.md 4.7) и
--     выделяется UPDATE ... RETURNING, то есть не из часов и без гонки.
-- Строки появляются лениво, когда панель впервые считает локатор: секрет
-- HMAC живёт в окружении, и миграция его не знает.

CREATE TABLE IF NOT EXISTS csm_subscriptions (
    subscription_id BIGINT      PRIMARY KEY REFERENCES subscriptions (id) ON DELETE CASCADE,
    -- Поколение локатора, с единицы. Инкремент отзывает старый локатор.
    gen             INTEGER     NOT NULL DEFAULT 1,
    -- base32 Crockford, 24 символа, каноническое написание.
    locator         TEXT        NOT NULL,
    -- Последняя выданная версия директивы; следующая это +1.
    directive_ver   BIGINT      NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_csm_subscriptions_locator
    ON csm_subscriptions (locator);
