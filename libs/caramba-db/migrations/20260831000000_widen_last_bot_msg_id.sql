-- users.last_bot_msg_id was created as INTEGER, but the User model reads it as
-- Option<i64>. Every `query_as::<_, User>` therefore failed at runtime with
-- "mismatched types; Rust type Option<i64> (as SQL type INT8) is not compatible
-- with SQL type INT4" — which broke subscription extension and any other path
-- that loads a whole user row (the repository layer hid it behind an i32
-- fallback, so only the direct query_as call sites were affected).
--
-- Widening is lossless and matches the model. Telegram message ids are also
-- free to outgrow i32.
ALTER TABLE users
    ALTER COLUMN last_bot_msg_id TYPE BIGINT;
