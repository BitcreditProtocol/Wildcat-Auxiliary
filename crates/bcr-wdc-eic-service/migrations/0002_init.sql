-- company_node_id is '' for personal (non-company) entries, so it can be part of the primary key
CREATE TABLE IF NOT EXISTS email_confirmations (
    node_id         TEXT NOT NULL,
    company_node_id TEXT NOT NULL,
    wrong_entries   INTEGER NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    blob            JSONB NOT NULL,
    PRIMARY KEY (node_id, company_node_id)
);

CREATE TABLE IF NOT EXISTS email_registrations (
    node_id         TEXT NOT NULL,
    company_node_id TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    blob            JSONB NOT NULL,
    PRIMARY KEY (node_id, company_node_id)
);
