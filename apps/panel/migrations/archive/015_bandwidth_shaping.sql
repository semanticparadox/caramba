-- Migration: 015_bandwidth_shaping.sql
-- Add configuration columns for Smart Bandwidth Shaping

-- 1. QoS Enabled: Prioritize real-time traffic (Gaming/VoIP)
ALTER TABLE nodes ADD COLUMN config_qos_enabled BOOLEAN DEFAULT 0;

-- 2. Block BitTorrent: Prevent P2P traffic
ALTER TABLE nodes ADD COLUMN config_block_torrent BOOLEAN DEFAULT 0;

-- 3. Block Ads: Use geosite:category-ads-all
ALTER TABLE nodes ADD COLUMN config_block_ads BOOLEAN DEFAULT 0;

-- 4. Block Porn: Use geosite:category-porn
ALTER TABLE nodes ADD COLUMN config_block_porn BOOLEAN DEFAULT 0;
