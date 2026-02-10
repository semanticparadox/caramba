-- Add capacity fields to nodes
ALTER TABLE nodes ADD COLUMN speed_limit_mbps INT DEFAULT 0;
ALTER TABLE nodes ADD COLUMN max_users INT DEFAULT 1000;
ALTER TABLE nodes ADD COLUMN current_speed_mbps INT DEFAULT 0;
