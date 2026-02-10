-- Add parent_id to users for Family Plans
ALTER TABLE users ADD COLUMN parent_id INTEGER DEFAULT NULL REFERENCES users(id);
CREATE INDEX idx_users_parent_id ON users(parent_id);
