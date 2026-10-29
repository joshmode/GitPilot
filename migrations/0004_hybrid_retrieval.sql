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

INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (4, datetime('now'));
