CREATE TABLE IF NOT EXISTS github_projects (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    owner_login TEXT NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL,
    closed INTEGER NOT NULL DEFAULT 0,
    synced_at TEXT NOT NULL,
    UNIQUE(account_id, owner_login, number)
);

CREATE TABLE IF NOT EXISTS github_project_items (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES github_projects(id) ON DELETE CASCADE,
    content_type TEXT NOT NULL,
    content_id TEXT,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    url TEXT,
    repository_name TEXT,
    updated_at TEXT,
    synced_at TEXT NOT NULL,
    UNIQUE(project_id, id)
);

CREATE TABLE IF NOT EXISTS webhook_events (
    delivery_id TEXT PRIMARY KEY,
    event TEXT NOT NULL,
    action TEXT,
    account_id TEXT,
    repository_full_name TEXT,
    received_at TEXT NOT NULL,
    processed_at TEXT,
    payload_sha256 TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_projects_account ON github_projects(account_id, synced_at);
CREATE INDEX IF NOT EXISTS idx_project_items_project ON github_project_items(project_id, state);
CREATE INDEX IF NOT EXISTS idx_webhook_received ON webhook_events(received_at);

INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'));
