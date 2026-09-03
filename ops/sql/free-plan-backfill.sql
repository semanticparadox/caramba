-- Бэкфилл бесплатного плана. Запускать ТОЛЬКО после того, как в бою уже стоит
-- бинарник с исправленной выдачей, и только после свежего pg_dump.
--
-- Почему бэкфилл вообще нужен. Выдача бесплатного плана в боте висит на приёме
-- условий, а он срабатывает один раз в жизни аккаунта. Все существующие
-- пользователи условия уже приняли, поэтому новый код до них не дотянется
-- никогда — включая пятерых, у которых нет ни одной подписки.
--
-- Почему после выката, а не до. До выката вставка ушла бы без vless_uuid, а
-- генерация конфигов нод пропускает такие строки молча: подписка была бы, а
-- подключиться человек всё равно не смог бы. Ровно этот баг мы и чиним.

\set ON_ERROR_STOP on

BEGIN;

-- 1. Проверка перед записью. Ожидаем ровно тех, у кого НЕТ НИ ОДНОЙ подписки.
--    Смотреть глазами, прежде чем идти дальше.
SELECT u.id, u.tg_id, u.email, u.terms_accepted_at
FROM users u
LEFT JOIN subscriptions s ON s.user_id = u.id
WHERE s.id IS NULL
ORDER BY u.id;

-- 2. Выдача. Условие `s.id IS NULL` — это и есть защита: строка не может
--    зацепить человека, у которого есть ХОТЬ КАКАЯ-ТО подписка, поэтому ни один
--    платящий подписчик недосягаем независимо от плана.
--
--    Бесплатный план ищется так же, как его ищет код, а не по имени и не по id:
--    оператор мог завести другой.
INSERT INTO subscriptions
    (user_id, plan_id, status, expires_at, subscription_uuid, vless_uuid,
     used_traffic, activated_at, created_at)
SELECT u.id,
       (SELECT id FROM plans WHERE is_free = TRUE AND is_active = TRUE ORDER BY id LIMIT 1),
       'active',
       '9999-12-31 23:59:59+00',
       gen_random_uuid()::TEXT,
       gen_random_uuid()::TEXT,
       0,
       CURRENT_TIMESTAMP,
       CURRENT_TIMESTAMP
FROM users u
LEFT JOIN subscriptions s ON s.user_id = u.id
WHERE s.id IS NULL
  AND EXISTS (SELECT 1 FROM plans WHERE is_free = TRUE AND is_active = TRUE);

-- 3. Лечение строк без vless_uuid.
--
--    Ограничение по бесплатному плану здесь не косметика. Без него строка
--    ротировала бы учётные данные ЛЮБОЙ подписки с пустым полем, в том числе
--    платной: у человека разом перестали бы работать все установленные
--    конфиги, и он бы не понял почему. Сегодня таких платных строк нет, но
--    условие защищает от того дня, когда появятся.
UPDATE subscriptions s
SET vless_uuid = gen_random_uuid()::TEXT
WHERE (s.vless_uuid IS NULL OR s.vless_uuid = '')
  AND s.status IN ('active', 'throttled')
  AND EXISTS (SELECT 1 FROM plans p WHERE p.id = s.plan_id AND p.is_free);

-- 4. Проверки после записи. Любая непройденная — повод откатить транзакцию.
--    Число платных подписок обязано остаться прежним: это и есть утверждение
--    «платящих не тронули».
SELECT
    (SELECT count(*) FROM users u
       LEFT JOIN subscriptions s ON s.user_id = u.id
      WHERE s.id IS NULL)                                  AS must_be_zero_users_without_sub,
    (SELECT count(*) FROM subscriptions s JOIN plans p ON p.id = s.plan_id
      WHERE p.is_free)                                     AS free_subs_now,
    (SELECT count(*) FROM subscriptions s JOIN plans p ON p.id = s.plan_id
      WHERE NOT p.is_free)                                 AS paid_subs_must_be_13,
    (SELECT count(*) FROM subscriptions
      WHERE vless_uuid IS NULL OR vless_uuid = '')         AS must_be_zero_without_vless;

-- Если цифры сошлись — COMMIT. Если нет — ROLLBACK.
-- Специально НЕ коммитим автоматически: это чужие деньги.
