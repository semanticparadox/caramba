-- Добавляем поля управления ротацией SNI на уровне ноды.
--
-- last_sni_rotation: метка времени последней ротации SNI на данной ноде.
--   Используется monitoring.rs для плановой глобальной ротации и
--   панельным UI для отображения когда последний раз менялся SNI.
--
-- sni_renew_interval_hours: период плановой ротации в часах.
--   0 = никогда (автоматическая ротация отключена для этой ноды).
--   NULL = использовать глобальную настройку auto_sni_rotation_interval_hours.
--   24 = раз в сутки, 168 = раз в неделю, 720 = раз в месяц.

ALTER TABLE nodes
    ADD COLUMN IF NOT EXISTS last_sni_rotation TIMESTAMPTZ DEFAULT NULL,
    ADD COLUMN IF NOT EXISTS sni_renew_interval_hours INTEGER DEFAULT NULL;

CREATE INDEX IF NOT EXISTS idx_nodes_last_sni_rotation ON nodes (last_sni_rotation);
