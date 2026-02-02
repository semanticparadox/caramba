-- Add payments table
CREATE TABLE IF NOT EXISTS payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    amount INTEGER NOT NULL, -- in cents
    method TEXT NOT NULL,
    status TEXT NOT NULL, -- 'pending', 'completed', 'failed'
    created_at INTEGER NOT NULL, -- timestamp
    updated_at INTEGER NOT NULL,
    transaction_id TEXT, -- external ID
    FOREIGN KEY(user_id) REFERENCES users(id)
);

-- Add referral columns to users table if they don't exist
-- SQLite doesn't support IF NOT EXISTS for columns, so we wrap in a benign block for raw SQL,
-- or just rely on 'ADD COLUMN' to fail if exists, but in migration script we expect strict order.
-- However, since I can't guarantee previous migrations had this, I'll use standard ALTER TABLE.
-- If I re-run this locally, it might fail if already exists.
-- A clearer way for SQLite migrations:
SELECT count(*) FROM pragma_table_info('users') WHERE name='referral_code';
-- But we can't condition inside SQL script easily without weird tricks.
-- I'll use separate statements and assume they run sequentially. 
-- In production, the migration runner tracks applied migrations, so 012 won't run twice.

ALTER TABLE users ADD COLUMN referral_code TEXT;
ALTER TABLE users ADD COLUMN referred_by INTEGER REFERENCES users(id);

-- Create index for referrals
CREATE INDEX IF NOT EXISTS idx_users_referral_code ON users(referral_code);
CREATE INDEX IF NOT EXISTS idx_users_referred_by ON users(referred_by);
