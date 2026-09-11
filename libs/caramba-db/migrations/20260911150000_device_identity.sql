-- Устройство принадлежит аккаунту, а не строке подписки (раунд 5, волна W2b, задача B2).
--
-- Зачем. Отпечаток устройства до сих пор считался от subscription_id
-- (`sha256("sub:{id}|ua:{ua}")`), а сама лиза ссылалась только на подписку.
-- Пока строк подписки на человека было несколько, это означало, что при любой
-- смене тарифа все привязки устройств исчезали: новая строка — новый отпечаток,
-- и телефон, с которого человек ходил год, снова считался новым и упирался в
-- лимит. Инвариант «одна активная подписка» (миграция 20260911140000) уже
-- свёл лизы на одну строку и переносит их триггером, но сама идентичность
-- устройства всё ещё выводится из подписки.
--
-- Эта миграция добавляет то, чего не хватает, чтобы устройство опознавалось
-- само по себе:
--   user_id          — владелец лизы; по нему считается лимит и строится список
--                      устройств в кабинете (раньше это был JOIN на подписку);
--   client_device_id — стабильный идентификатор, который наше приложение
--                      генерирует один раз и присылает заголовком
--                      X-Caramba-Device-Id; переживает смену User-Agent при
--                      обновлении приложения и смену сети;
--   platform         — «android»/«ios»/«macos»/… из заголовка, чтобы в списке
--                      устройств была не только строка UA.
--
-- Сторонние клиенты (Hiddify, Clash, v2rayNG) своего идентификатора прислать не
-- могут — для них остаётся отпечаток по User-Agent, но теперь он считается от
-- user_id, а не от subscription_id.
--
-- Миграция аддитивная: ничего не удаляет и не переименовывает, порядок
-- относительно 20260911140000 соблюдён (к этому моменту лизы уже
-- консолидированы на одной подписке пользователя).

ALTER TABLE subscription_device_leases
    ADD COLUMN IF NOT EXISTS user_id BIGINT REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE subscription_device_leases
    ADD COLUMN IF NOT EXISTS client_device_id TEXT;

ALTER TABLE subscription_device_leases
    ADD COLUMN IF NOT EXISTS platform TEXT;

-- Backfill владельца из подписки: до этой миграции он выводился JOIN-ом, теперь
-- хранится.
UPDATE subscription_device_leases l
SET user_id = s.user_id
FROM subscriptions s
WHERE s.id = l.subscription_id
  AND l.user_id IS DISTINCT FROM s.user_id;

-- Владельца проставляет база, а не вызывающий код.
--
-- Лизу двигает не только панель: триггер trg_subscriptions_carry_leases
-- (миграция 20260911140000) переносит её на новую подписку при смене тарифа, и
-- любой такой перенос обязан приводить user_id в соответствие. Держать это в
-- коде означало бы полагаться на то, что каждый будущий UPDATE вспомнит про
-- колонку; здесь же рассинхронизация невозможна по построению.
CREATE OR REPLACE FUNCTION device_leases_fill_user()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.user_id IS NULL OR TG_OP = 'UPDATE' THEN
        SELECT s.user_id INTO NEW.user_id
        FROM subscriptions s
        WHERE s.id = NEW.subscription_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_device_leases_fill_user ON subscription_device_leases;

CREATE TRIGGER trg_device_leases_fill_user
    BEFORE INSERT OR UPDATE OF subscription_id ON subscription_device_leases
    FOR EACH ROW
    EXECUTE FUNCTION device_leases_fill_user();

-- Список устройств кабинета и счётчик лимита теперь читают по владельцу.
CREATE INDEX IF NOT EXISTS idx_subscription_device_leases_user
    ON subscription_device_leases(user_id);

-- Поиск «эта же лиза» при каждом обращении за подпиской: пара
-- (владелец, идентификатор приложения). Индекс частичный — у сторонних
-- клиентов client_device_id всегда NULL, и такие строки в нём не нужны.
--
-- Индекс намеренно НЕ уникальный. Уникальность здесь уронила бы перенос лиз
-- триггером смены подписки: он дедуплицирует строки по device_fingerprint, про
-- client_device_id не знает, и две легаси-лизы одного устройства (появившиеся
-- до этой миграции, с разными отпечатками) сложились бы в конфликт прямо
-- посреди выдачи подписки. Единственность обеспечивает панель: она ищет
-- существующую лизу по этой паре и обновляет её, а не вставляет вторую.
CREATE INDEX IF NOT EXISTS idx_subscription_device_leases_user_device
    ON subscription_device_leases(user_id, client_device_id)
    WHERE client_device_id IS NOT NULL;
