-- Инвариант «одна активная подписка на пользователя» (раунд 5, волна W2b, задача B1).
--
-- Зачем. До сих пор каждый путь выдачи (покупка другого тарифа, подарок админа,
-- gift/промо-код) вставлял НОВУЮ строку подписки, не трогая старую: в коде это
-- прямо записано комментариями «Never touches existing subs». В итоге у одного
-- человека спокойно жили две строки status='active' — вечная бесплатная
-- (expires_at = 9999 год) и купленная платная. Дальше расходились все четыре
-- независимые реализации выбора «активной» подписки: одни брали
-- ORDER BY expires_at DESC и всегда попадали в бесплатную (9999 год выигрывает
-- у любой реальной даты), другие — самую старую по id. Отсюда и «путаница с
-- подписками»: бот, мини-апп и учёт трафика обслуживали разные строки одного
-- аккаунта. Побочно ломались устройства: device_fingerprint зашит на
-- subscription_id, поэтому появление второй строки обнуляло все привязки.
--
-- Миграция аддитивная: ничего не удаляет и не переименовывает. Три шага —
-- (1) привести существующие данные, (2) закрыть инвариант уникальным индексом,
-- (3) поставить триггер-страховку, который переводит прежнюю активную строку в
-- 'superseded' ВМЕСТО того, чтобы уронить чужой UPDATE об уникальный индекс.
--
-- Статус 'superseded' намеренно отличается от 'expired': истечение по сроку
-- обрабатывается мониторингом и возвращает человека на бесплатный план, а
-- 'superseded' означает «строку заменили другой, живой» — её нельзя ни
-- реанимировать по сроку, ни считать потерей доступа.

-- ---------------------------------------------------------------------------
-- Шаг 1. Существующие дубли: оставляем ОДНУ активную строку на пользователя.
--
-- Порядок выбора главной строки един для всей системы (см.
-- subscription_repo::ACTIVE_SUBSCRIPTION_ORDER_SQL): платная важнее бесплатной,
-- среди равных — та, что сгорает раньше (её потеря заметнее), при полном
-- равенстве — меньший id. Лизы устройств переезжают на оставшуюся строку,
-- иначе привязки устройств исчезли бы вместе со статусом.
-- ---------------------------------------------------------------------------

DROP TABLE IF EXISTS _single_active_losers;

CREATE TEMP TABLE _single_active_losers AS
WITH ranked AS (
    SELECT
        s.id,
        s.user_id,
        ROW_NUMBER() OVER (
            PARTITION BY s.user_id
            ORDER BY COALESCE(p.is_free, FALSE) ASC, s.expires_at ASC, s.id ASC
        ) AS rn
    FROM subscriptions s
    LEFT JOIN plans p ON p.id = s.plan_id
    WHERE s.status = 'active'
),
keepers AS (
    SELECT user_id, id FROM ranked WHERE rn = 1
)
SELECT r.id AS loser_id, k.id AS keeper_id
FROM ranked r
JOIN keepers k ON k.user_id = r.user_id
WHERE r.rn > 1;

-- Переносим лизы устройств на оставшуюся подписку. UNIQUE(subscription_id,
-- device_fingerprint) требует, чтобы на keeper'а приехала ровно одна лиза на
-- отпечаток: поэтому DISTINCT ON выбирает самую свежую, а всё остальное (и уже
-- имеющиеся у keeper'а дубли, и проигравшие копии) убирает следующий запрос.
-- Без DISTINCT ON два вытесняемых подписки с одинаковым отпечатком роняли бы
-- перенос об уникальный индекс: проверка NOT EXISTS не видит строки, которые
-- этот же UPDATE переносит прямо сейчас.
UPDATE subscription_device_leases l
SET subscription_id = lo.keeper_id
FROM _single_active_losers lo
WHERE l.subscription_id = lo.loser_id
  AND l.id IN (
      SELECT DISTINCT ON (x.keeper_id, l2.device_fingerprint) l2.id
      FROM subscription_device_leases l2
      JOIN _single_active_losers x ON x.loser_id = l2.subscription_id
      WHERE NOT EXISTS (
          SELECT 1 FROM subscription_device_leases t
          WHERE t.subscription_id = x.keeper_id
            AND t.device_fingerprint = l2.device_fingerprint
      )
      ORDER BY x.keeper_id, l2.device_fingerprint, l2.last_seen_at DESC, l2.id DESC
  );

DELETE FROM subscription_device_leases l
USING _single_active_losers lo
WHERE l.subscription_id = lo.loser_id;

UPDATE subscriptions s
SET status = 'superseded'
FROM _single_active_losers lo
WHERE s.id = lo.loser_id;

DROP TABLE IF EXISTS _single_active_losers;

-- ---------------------------------------------------------------------------
-- Шаг 2. Сам инвариант.
-- ---------------------------------------------------------------------------

CREATE UNIQUE INDEX IF NOT EXISTS uq_subscriptions_single_active
    ON subscriptions (user_id)
    WHERE status = 'active';

-- ---------------------------------------------------------------------------
-- Шаг 3. Триггер-страховка.
--
-- Панель переводит подписку в 'active' не только через единый сервисный метод:
-- есть админское продление (subscription_service::admin_extend), одобрение
-- pending-подписки, ночное снятие троттлинга в мониторинге. Любой из этих
-- путей, встретив вторую активную строку, упёрся бы в уникальный индекс и
-- вернул 500 (а в случае пакетного UPDATE в мониторинге — уронил бы весь цикл).
-- Поэтому инвариант держится не отказом, а вытеснением: строка, ставшая
-- активной, гасит остальные активные строки того же пользователя.
--
-- Рекурсии нет: вложенный UPDATE ставит 'superseded', а условие WHEN у триггера
-- срабатывает только на 'active'.
--
-- user_id в списке колонок не случайно: передача подписки другому аккаунту
-- (subscription_service::transfer) меняет владельца, не трогая статус, и без
-- этого пункта активная подписка могла бы переехать к человеку, у которого
-- активная уже есть.
-- ---------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION subscriptions_enforce_single_active()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE subscriptions
    SET status = 'superseded'
    WHERE user_id = NEW.user_id
      AND id IS DISTINCT FROM NEW.id
      AND status = 'active';
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_subscriptions_single_active ON subscriptions;

CREATE TRIGGER trg_subscriptions_single_active
    BEFORE INSERT OR UPDATE OF status, user_id ON subscriptions
    FOR EACH ROW
    WHEN (NEW.status = 'active')
    EXECUTE FUNCTION subscriptions_enforce_single_active();

-- Вторая половина того же инварианта: привязки устройств.
--
-- Отпечаток устройства считается от subscription_id, поэтому любая замена
-- подписки без переноса лиз молча отвязывает все устройства человека — при том
-- что по постановке устройство привязывается к аккаунту и держится до ручной
-- отвязки. Перенос обязан идти ПОСЛЕ вставки: в BEFORE-триггере строки NEW ещё
-- нет в таблице, и внешний ключ лизы на неё сослаться не может.
--
-- Собираем лизы со ВСЕХ прочих подписок пользователя, а не только со свежее
-- вытесненной. Статусов, с которых устройство должно вернуться, два:
-- 'superseded' (сменили тариф) и 'expired' (платная кончилась, человек упал на
-- бесплатную) — и оба означают одно: устройства принадлежат аккаунту, а не
-- строке подписки. Инвариант «одна активная» делает эту выборку однозначной:
-- кроме NEW активных строк не остаётся.
--
-- Лиза, для которой на действующей подписке уже есть такой же отпечаток,
-- переехать не может (UNIQUE) и удаляется как точный дубль.
CREATE OR REPLACE FUNCTION subscriptions_carry_device_leases()
RETURNS TRIGGER AS $$
DECLARE
    other_ids BIGINT[];
BEGIN
    SELECT array_agg(id) INTO other_ids
    FROM subscriptions
    WHERE user_id = NEW.user_id
      AND id IS DISTINCT FROM NEW.id
      AND status <> 'active';

    IF other_ids IS NULL THEN
        RETURN NULL;
    END IF;

    -- DISTINCT ON: на новую подписку обязана приехать ровно одна лиза на
    -- отпечаток (UNIQUE), берём самую свежую. NOT EXISTS сам по себе не
    -- спасает — он не видит строк, которые этот же UPDATE переносит сейчас.
    UPDATE subscription_device_leases l
    SET subscription_id = NEW.id
    WHERE l.id IN (
        SELECT DISTINCT ON (l2.device_fingerprint) l2.id
        FROM subscription_device_leases l2
        WHERE l2.subscription_id = ANY(other_ids)
          AND NOT EXISTS (
              SELECT 1 FROM subscription_device_leases t
              WHERE t.subscription_id = NEW.id
                AND t.device_fingerprint = l2.device_fingerprint
          )
        ORDER BY l2.device_fingerprint, l2.last_seen_at DESC, l2.id DESC
    );

    DELETE FROM subscription_device_leases
    WHERE subscription_id = ANY(other_ids);

    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_subscriptions_carry_leases ON subscriptions;

CREATE TRIGGER trg_subscriptions_carry_leases
    AFTER INSERT OR UPDATE OF status, user_id ON subscriptions
    FOR EACH ROW
    WHEN (NEW.status = 'active')
    EXECUTE FUNCTION subscriptions_carry_device_leases();
