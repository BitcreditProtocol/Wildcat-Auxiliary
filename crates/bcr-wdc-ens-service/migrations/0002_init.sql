-- company_node_id is '' for personal (non-company) entries, so it can be part of the primary key
CREATE TABLE IF NOT EXISTS email_notification_preferences (
    node_id         TEXT NOT NULL,
    company_node_id TEXT NOT NULL,
    pref_token      UUID NOT NULL UNIQUE,
    enabled         BOOLEAN NOT NULL,
    preferences     BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    blob            JSONB NOT NULL,
    PRIMARY KEY (node_id, company_node_id)
);
