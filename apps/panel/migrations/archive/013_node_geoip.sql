-- Add GeoIP coordinates to nodes
ALTER TABLE nodes ADD COLUMN latitude REAL;
ALTER TABLE nodes ADD COLUMN longitude REAL;
