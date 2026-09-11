-- Обновления клиента Caramba Connect (раунд 6, задача B).
--
-- Зачем. У системы не было понятия «версия установленного клиента»: клиент
-- её не сообщал, панели негде было хранить, бот не мог сказать «вышла новая
-- версия». Эта миграция даёт две вещи:
--
--   1. subscription_device_leases.app_version — версия приложения на
--      устройстве, как оно назвалось заголовком X-Caramba-App-Version
--      («1.0.0+110»). Только для показа в кабинете и админке; решения о
--      доступе по ней не принимаются (заголовок — недоверенный вход).
--
--   2. client_release_notices — журнал разосланных версий. Бот шлёт «вышла
--      новая версия» ОДИН раз на сборку (build) — гарантию даёт первичный
--      ключ: цикл сначала вставляет строку (ON CONFLICT DO NOTHING) и шлёт
--      только если вставил. Логика — services/client_release_service.rs.
--
-- Бэкфилл: сборка 109 (клиент на проде в момент миграции) на всех четырёх
-- платформах записывается как уже разосланная. Без этого первый проход цикла
-- увидел бы манифест 1.0.0+109 в downloads/ и разослал бы «вышла версия
-- 1.0.0» всей базе про версию, которая у всех уже стоит. Сам сервис страхует
-- это ещё раз: если таблица пуста, первый увиденный манифест записывается
-- молча.
--
-- Миграция аддитивная: новая колонка, новая таблица, настройки с дефолтами.

ALTER TABLE subscription_device_leases
    ADD COLUMN IF NOT EXISTS app_version TEXT;

CREATE TABLE IF NOT EXISTS client_release_notices (
    -- android | windows | macos | linux
    platform    TEXT        NOT NULL,
    -- номер сборки из pubspec (+N), сравнивается численно
    build       BIGINT      NOT NULL,
    -- X.Y.Z для журнала и текста уведомления
    version     TEXT        NOT NULL DEFAULT '',
    -- сколько сообщений ушло в бот (0 — записано молча: бэкфилл или
    -- рассылка выключена)
    sent_count  BIGINT      NOT NULL DEFAULT 0,
    notified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (platform, build)
);

INSERT INTO client_release_notices (platform, build, version, sent_count)
VALUES ('android', 109, '1.0.0', 0),
       ('windows', 109, '1.0.0', 0),
       ('macos', 109, '1.0.0', 0),
       ('linux', 109, '1.0.0', 0)
ON CONFLICT (platform, build) DO NOTHING;

-- Настройки блока «Client updates» в админке Settings.
--   client_update_notify — рассылать ли «вышла новая версия» автоматически;
--   client_min_build     — минимальная сборка (0 = не требовать); ниже неё
--                          приложение показывает «Нужно обновиться» и не
--                          пускает дальше. Платформенные client_min_build_<p>
--                          сильнее глобальной и заводятся админкой по мере
--                          надобности;
--   client_release_notes — «что нового», уходит в уведомление и в баннер;
--   client_latest_version / client_latest_build — ручной запасной источник
--                          версии, когда манифестов в downloads/ ещё нет.
INSERT INTO settings (key, value)
VALUES ('client_update_notify', 'true'),
       ('client_min_build', '0'),
       ('client_release_notes', ''),
       ('client_latest_version', ''),
       ('client_latest_build', '')
ON CONFLICT (key) DO NOTHING;
