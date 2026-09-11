-- Онбординг новых пользователей (раунд 5, волна W5, задача G1).
--
-- Зачем. Бот шлёт три касания после регистрации: «Ваши следующие шаги» сразу,
-- «Вы ещё не подключились» через сутки (только без устройств) и «Что ещё
-- умеет Caramba Connect» через трое суток. Каждый шаг уходит один раз, и
-- гарантирует это первичный ключ этой таблицы: шаг сначала вставляется
-- (ON CONFLICT DO NOTHING), и отправляет только тот, кто вставил строку.
-- Логика — apps/caramba-panel/src/services/onboarding_service.rs.
--
-- Миграция аддитивная: новая таблица, три настройки с дефолтами, бэкфилл.

CREATE TABLE IF NOT EXISTS onboarding_deliveries (
    user_id BIGINT      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- day0 | day1 | day3 (onboarding_service::Step::id)
    step    TEXT        NOT NULL,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, step)
);

-- Бэкфилл: все, кто принял соглашение ДО релиза, считаются прошедшими
-- онбординг целиком. Без этого фоновый цикл в первые полчаса разослал бы
-- «вы ещё не подключились» каждому существующему аккаунту.
--
-- Только с terms_accepted_at: аккаунт, который нажал /start, но соглашение
-- ещё не принял, регистрацию не закончил. Когда он её закончит, ветка
-- accept_terms должна отправить ему day0 как новичку, а записанный здесь
-- шаг это бы заблокировал.
INSERT INTO onboarding_deliveries (user_id, step, sent_at)
SELECT u.id, s.step, NOW()
FROM users u
CROSS JOIN (VALUES ('day0'), ('day1'), ('day3')) AS s(step)
WHERE u.terms_accepted_at IS NOT NULL
ON CONFLICT (user_id, step) DO NOTHING;

-- Настройки цикла (админка Settings → «Онбординг»). Включён по умолчанию:
-- это не рассылка, а часть регистрации.
INSERT INTO settings (key, value)
VALUES ('onboarding_enabled', 'true'),
       ('onboarding_day1_hours', '24'),
       ('onboarding_day3_hours', '72')
ON CONFLICT (key) DO NOTHING;
