-- Добавляем поле display_name для пользовательских имён устройств.
-- Оставляем auto-generated device_name нетронутым — это fallback.
-- IF NOT EXISTS — безопасно при повторном запуске.
ALTER TABLE subscription_device_leases
    ADD COLUMN IF NOT EXISTS display_name TEXT;
