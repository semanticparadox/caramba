-- ================================================
-- АККАУНТЫ STANDALONE-ПРИЛОЖЕНИЯ (email/password + Telegram-login)
-- ------------------------------------------------
-- Новое кросс-платформенное супер-приложение (Flutter + Go-ядро mihomo)
-- логинится напрямую в панель и тянет mihomo-конфиг подписки. Для этого
-- нужны email/password-аккаунты в дополнение к существующим Telegram-юзерам:
--   * tg_id становится NULLABLE (email-аккаунт может не иметь Telegram);
--   * добавляем email/password_hash/email_verified/auth_provider;
--   * refresh_tokens хранит хешированные refresh-токены (ротация/отзыв).
-- ================================================

-- Telegram ID больше не обязателен: аккаунт может быть создан через email.
ALTER TABLE users ALTER COLUMN tg_id DROP NOT NULL;

-- Поля для email/password-аутентификации.
ALTER TABLE users ADD COLUMN IF NOT EXISTS email TEXT UNIQUE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_hash TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS email_verified BOOLEAN DEFAULT FALSE;
-- Способ регистрации: 'telegram' | 'email' (для аналитики/логики восстановления).
ALTER TABLE users ADD COLUMN IF NOT EXISTS auth_provider TEXT;

-- Регистронезависимый поиск по email (логин не должен зависеть от регистра).
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_lower
    ON users (LOWER(email))
    WHERE email IS NOT NULL;

-- ------------------------------------------------
-- REFRESH-ТОКЕНЫ
-- В БД хранится ТОЛЬКО хеш токена (sha256-hex), как и у refresh-секретов
-- в других местах — утечка таблицы не раскрывает живые токены.
-- ------------------------------------------------
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked BOOLEAN DEFAULT FALSE,
    user_agent TEXT,
    created_at TIMESTAMPTZ DEFAULT now()
);

-- Поиск токена при refresh/logout идёт по хешу.
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);
-- Массовый отзыв всех сессий пользователя (logout-all / бан).
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user_id ON refresh_tokens(user_id);
