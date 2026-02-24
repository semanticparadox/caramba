-- Drop the FK constraint since product_id can refer to either a Plan OR a Store Product
-- This fixes the foreign key violation when purchasing a Plan from the Miniapp
ALTER TABLE payment_sessions DROP CONSTRAINT IF EXISTS payment_sessions_product_id_fkey;
