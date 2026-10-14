
CREATE TABLE IF NOT EXISTS challenges (
    node_id    TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    blob       JSONB NOT NULL
);
