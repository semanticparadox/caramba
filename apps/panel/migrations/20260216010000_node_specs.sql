-- Add hardware specs to nodes table
ALTER TABLE nodes ADD COLUMN max_ram BIGINT DEFAULT 0;
ALTER TABLE nodes ADD COLUMN cpu_cores INTEGER DEFAULT 0;
ALTER TABLE nodes ADD COLUMN cpu_model TEXT;
