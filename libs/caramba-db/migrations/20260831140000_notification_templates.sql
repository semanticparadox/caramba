-- Editable notification templates.
--
-- The nine `notify.*` messages are the ones that carry money: a subscription
-- expired, a balance running low, an auto-renewal that failed. Until now they
-- were `macro_rules!` string literals compiled into the binary — changing a
-- comma meant a rebuild and a redeploy — and every one of them went out as
-- plain text, so the copy said "top up your balance" with nothing to tap.
--
-- This table holds per-event, per-language OVERRIDES. It is deliberately not a
-- copy of the defaults:
--
--   * no row, or a NULL column, means "use the built-in string". So an operator
--     who never opened the editor keeps receiving improvements to the wording
--     that ship with the code, and "reset to default" is a DELETE rather than a
--     re-insert of whatever the default happened to be on that day.
--
--   * the payload columns mirror `NotificationPayload` (bot_manager.rs) and the
--     broadcast composer that already exists on the notifications page, so one
--     shape describes a one-off broadcast and a system notification alike.
--
-- `lang` is the same two-value domain as `bot::translations::Lang` ('ru' | 'en'),
-- kept as TEXT because that enum lives in Rust and a Postgres enum would have to
-- be migrated in lockstep with it for no gain.

CREATE TABLE IF NOT EXISTS notification_templates (
    event                TEXT        NOT NULL,
    lang                 TEXT        NOT NULL,
    -- NULL = fall back to the compiled-in string for this field.
    text                 TEXT,
    title                TEXT,
    body                 TEXT,
    parse_mode           TEXT        NOT NULL DEFAULT 'html',
    media_type           TEXT        NOT NULL DEFAULT 'none',
    media_url            TEXT,
    -- [{"text": "...", "url": "..."}, ...] — same order the keyboard renders in.
    buttons_json         JSONB,
    disable_link_preview BOOLEAN     NOT NULL DEFAULT FALSE,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_by           TEXT,
    PRIMARY KEY (event, lang)
);

-- The short name of the Telegram Mini App, used to build the deep link a
-- notification button points at: https://t.me/<bot>/<short_name>?startapp=<screen>.
-- Empty means "not configured": the button then points at the bot itself rather
-- than being assembled into a broken URL.
INSERT INTO settings (key, value)
VALUES ('mini_app_short_name', '')
ON CONFLICT (key) DO NOTHING;
