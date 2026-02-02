-- Add telemetry columns to nodes table
ALTER TABLE nodes ADD COLUMN last_latency FLOAT; -- Ping to Cloudflare in ms
ALTER TABLE nodes ADD COLUMN last_cpu FLOAT; -- CPU Usage %
ALTER TABLE nodes ADD COLUMN last_ram FLOAT; -- RAM Usage %
