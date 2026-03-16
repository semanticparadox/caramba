ALTER TABLE nodes
    ADD COLUMN IF NOT EXISTS node_type TEXT;

UPDATE nodes
SET node_type = CASE WHEN is_relay THEN 'relay' ELSE 'exit' END
WHERE node_type IS NULL OR trim(node_type) = '';

UPDATE nodes
SET node_type = 'exit'
WHERE node_type NOT IN ('exit', 'relay');

ALTER TABLE nodes
    ALTER COLUMN node_type SET DEFAULT 'exit';

ALTER TABLE nodes
    ALTER COLUMN node_type SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_nodes_node_type ON nodes(node_type);
