ALTER TABLE pull_requests ADD COLUMN commit_count INTEGER NOT NULL DEFAULT 0;
INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'));
