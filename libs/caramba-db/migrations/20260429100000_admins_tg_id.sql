-- Adds the `tg_id` column on `admins`, which the panel's tickets and orgs
-- handlers already query (assignee LEFT JOIN admins ON admins.tg_id, plus
-- resolve_admin_tg_id by username -> tg_id). Without this column those
-- queries throw "column a.tg_id does not exist", breaking the entire
-- /admin/tickets feature at runtime.

ALTER TABLE admins
    ADD COLUMN IF NOT EXISTS tg_id BIGINT UNIQUE;

-- Best-effort backfill for installs where the admin already exists as a
-- regular user (matched by username). Newer installs seed admins via the
-- /setup flow which can write tg_id directly.
UPDATE admins a
SET    tg_id = u.tg_id
FROM   users u
WHERE  a.tg_id IS NULL
  AND  u.username IS NOT NULL
  AND  lower(u.username) = lower(a.username);

CREATE INDEX IF NOT EXISTS idx_admins_tg_id ON admins(tg_id) WHERE tg_id IS NOT NULL;
