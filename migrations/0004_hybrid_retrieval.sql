CREATE TABLE IF NOT EXISTS knowledge_embeddings (
    record_id TEXT NOT NULL REFERENCES knowledge_records(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    vector BLOB NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(record_id, provider_id, model)
);

CREATE INDEX IF NOT EXISTS idx_knowledge_embeddings_record
    ON knowledge_embeddings(record_id);

CREATE TABLE IF NOT EXISTS repository_branches (
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    head_sha TEXT NOT NULL,
    protected INTEGER NOT NULL DEFAULT 0,
    observed_at TEXT NOT NULL,
    PRIMARY KEY(repository_id, name)
);

CREATE INDEX IF NOT EXISTS idx_repository_branches_repo
    ON repository_branches(repository_id, name);

INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (4, datetime('now'));
