PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    login TEXT NOT NULL,
    host TEXT NOT NULL,
    avatar_url TEXT,
    connected_at TEXT NOT NULL,
    last_validated_at TEXT,
    UNIQUE(host, login)
);

CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    full_name TEXT NOT NULL,
    description TEXT,
    default_branch TEXT NOT NULL DEFAULT 'main',
    private INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    stars INTEGER NOT NULL DEFAULT 0,
    open_issues INTEGER NOT NULL DEFAULT 0,
    html_url TEXT NOT NULL,
    language TEXT,
    pushed_at TEXT,
    updated_at TEXT NOT NULL,
    favorite INTEGER NOT NULL DEFAULT 0,
    workspace TEXT,
    origin TEXT NOT NULL DEFAULT 'github',
    last_synced_at TEXT,
    etag_prs TEXT,
    etag_issues TEXT,
    security_state TEXT NOT NULL DEFAULT 'not_checked',
    UNIQUE(account_id, full_name)
);

CREATE TABLE IF NOT EXISTS pull_requests (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    repository_name TEXT NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL,
    state TEXT NOT NULL,
    draft INTEGER NOT NULL DEFAULT 0,
    merged INTEGER NOT NULL DEFAULT 0,
    mergeable TEXT NOT NULL DEFAULT 'unknown',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    merged_at TEXT,
    base_ref TEXT NOT NULL,
    head_ref TEXT NOT NULL,
    base_sha TEXT NOT NULL DEFAULT '',
    head_sha TEXT NOT NULL DEFAULT '',
    additions INTEGER NOT NULL DEFAULT 0,
    deletions INTEGER NOT NULL DEFAULT 0,
    changed_files INTEGER NOT NULL DEFAULT 0,
    commit_count INTEGER NOT NULL DEFAULT 0,
    review_state TEXT NOT NULL DEFAULT 'none',
    check_state TEXT NOT NULL DEFAULT 'missing',
    labels TEXT NOT NULL DEFAULT '[]',
    assignees TEXT NOT NULL DEFAULT '[]',
    requested_reviewers TEXT NOT NULL DEFAULT '[]',
    milestone TEXT,
    html_url TEXT NOT NULL,
    risk_score INTEGER NOT NULL DEFAULT 0,
    synced_at TEXT NOT NULL,
    origin TEXT NOT NULL DEFAULT 'github',
    UNIQUE(repository_id, number)
);

CREATE TABLE IF NOT EXISTS issues (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    repository_name TEXT NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    closed_at TEXT,
    labels TEXT NOT NULL DEFAULT '[]',
    assignees TEXT NOT NULL DEFAULT '[]',
    milestone TEXT,
    comments INTEGER NOT NULL DEFAULT 0,
    html_url TEXT NOT NULL,
    origin TEXT NOT NULL DEFAULT 'github',
    UNIQUE(repository_id, number)
);

CREATE TABLE IF NOT EXISTS milestones (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    open_issues INTEGER NOT NULL DEFAULT 0,
    closed_issues INTEGER NOT NULL DEFAULT 0,
    due_on TEXT,
    fetched_at TEXT NOT NULL,
    UNIQUE(repository_id, title)
);

CREATE TABLE IF NOT EXISTS security_alerts (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL,
    url TEXT,
    observed_at TEXT NOT NULL,
    UNIQUE(repository_id, kind, id)
);

CREATE TABLE IF NOT EXISTS branch_protection (
    repository_id TEXT PRIMARY KEY REFERENCES repositories(id) ON DELETE CASCADE,
    admins_enforced INTEGER NOT NULL DEFAULT 0,
    required_checks TEXT NOT NULL DEFAULT '[]',
    observed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS commits (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    repository_name TEXT NOT NULL,
    sha TEXT NOT NULL,
    message TEXT NOT NULL,
    author_name TEXT NOT NULL,
    author_login TEXT,
    authored_at TEXT NOT NULL,
    committed_at TEXT NOT NULL,
    parents TEXT NOT NULL DEFAULT '[]',
    additions INTEGER NOT NULL DEFAULT 0,
    deletions INTEGER NOT NULL DEFAULT 0,
    html_url TEXT NOT NULL,
    pr_number INTEGER,
    verification TEXT,
    origin TEXT NOT NULL DEFAULT 'github',
    UNIQUE(repository_id, sha)
);

CREATE TABLE IF NOT EXISTS checks (
    id TEXT PRIMARY KEY,
    pull_request_id TEXT REFERENCES pull_requests(id) ON DELETE CASCADE,
    commit_sha TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    conclusion TEXT,
    details_url TEXT,
    started_at TEXT,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS pull_request_files (
    pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    previous_path TEXT,
    status TEXT NOT NULL,
    additions INTEGER NOT NULL DEFAULT 0,
    deletions INTEGER NOT NULL DEFAULT 0,
    generated INTEGER NOT NULL DEFAULT 0,
    patch TEXT,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY(pull_request_id, path)
);

CREATE TABLE IF NOT EXISTS review_comments (
    id TEXT PRIMARY KEY,
    pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
    author TEXT NOT NULL,
    body TEXT NOT NULL,
    path TEXT,
    line INTEGER,
    diff_hunk TEXT,
    state TEXT,
    submitted_at TEXT NOT NULL,
    html_url TEXT
);

CREATE TABLE IF NOT EXISTS releases (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    tag_name TEXT NOT NULL,
    name TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    draft INTEGER NOT NULL DEFAULT 0,
    prerelease INTEGER NOT NULL DEFAULT 0,
    published_at TEXT,
    created_at TEXT NOT NULL,
    html_url TEXT NOT NULL,
    UNIQUE(repository_id, tag_name)
);

CREATE TABLE IF NOT EXISTS review_runs (
    id TEXT PRIMARY KEY,
    pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
    head_sha TEXT NOT NULL,
    status TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    deterministic_findings INTEGER NOT NULL DEFAULT 0,
    ai_findings INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0,
    error TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    UNIQUE(pull_request_id, head_sha)
);

CREATE TABLE IF NOT EXISTS review_findings (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES review_runs(id) ON DELETE CASCADE,
    pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    category TEXT NOT NULL,
    severity TEXT NOT NULL,
    confidence REAL NOT NULL,
    title TEXT NOT NULL,
    explanation TEXT NOT NULL,
    why_it_matters TEXT NOT NULL,
    evidence TEXT NOT NULL,
    path TEXT,
    diff_line INTEGER,
    suggested_remediation TEXT NOT NULL DEFAULT '',
    relevant_tests TEXT NOT NULL DEFAULT '[]',
    verification_status TEXT NOT NULL DEFAULT 'unverified',
    lifecycle TEXT NOT NULL DEFAULT 'unverified',
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(pull_request_id, fingerprint)
);

CREATE TABLE IF NOT EXISTS findings_feedback (
    finding_id TEXT PRIMARY KEY REFERENCES review_findings(id) ON DELETE CASCADE,
    verdict TEXT NOT NULL,
    note TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS knowledge_records (
    id TEXT PRIMARY KEY,
    repository_id TEXT REFERENCES repositories(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    source_object TEXT NOT NULL,
    source_version TEXT,
    source_url TEXT,
    confidence REAL NOT NULL DEFAULT 1.0,
    extraction_method TEXT NOT NULL,
    explicit_fact INTEGER NOT NULL DEFAULT 1,
    superseded_by TEXT,
    extracted_at TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    UNIQUE(repository_id, source_object, source_version, kind)
);

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    repository_scope TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS conversation_messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id TEXT PRIMARY KEY,
    repository_id TEXT REFERENCES repositories(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    read_at TEXT,
    snoozed_until TEXT,
    dedupe_key TEXT NOT NULL UNIQUE,
    url TEXT
);

CREATE TABLE IF NOT EXISTS provider_configs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    model TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    local_only INTEGER NOT NULL DEFAULT 0,
    input_cost_per_million REAL,
    output_cost_per_million REAL,
    context_limit INTEGER NOT NULL DEFAULT 128000,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS routing_policies (
    task TEXT PRIMARY KEY,
    primary_provider_id TEXT NOT NULL,
    fallback_provider_id TEXT,
    max_cost_usd REAL NOT NULL DEFAULT 0,
    max_input_tokens INTEGER NOT NULL DEFAULT 32000
);

CREATE TABLE IF NOT EXISTS provider_usage (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    task TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    retries INTEGER NOT NULL,
    estimated_cost_usd REAL NOT NULL,
    fallback_used INTEGER NOT NULL,
    cancelled INTEGER NOT NULL,
    succeeded INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY,
    occurred_at TEXT NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    outcome TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS automations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0,
    trigger TEXT NOT NULL,
    conditions TEXT NOT NULL DEFAULT '[]',
    actions TEXT NOT NULL DEFAULT '[]',
    dry_run INTEGER NOT NULL DEFAULT 1,
    requires_approval INTEGER NOT NULL DEFAULT 1,
    repository_id TEXT,
    policy TEXT NOT NULL,
    budget_usd REAL NOT NULL DEFAULT 0,
    timeout_seconds INTEGER NOT NULL DEFAULT 300,
    created_at TEXT NOT NULL,
    last_run_at TEXT
);

CREATE TABLE IF NOT EXISTS automation_runs (
    id TEXT PRIMARY KEY,
    automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
    event_key TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    error TEXT,
    UNIQUE(automation_id, event_key)
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    run_after TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error TEXT
);

CREATE TABLE IF NOT EXISTS local_repositories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    default_branch TEXT NOT NULL,
    connected_at TEXT NOT NULL,
    last_opened_at TEXT
);

CREATE TABLE IF NOT EXISTS saved_views (
    id TEXT PRIMARY KEY,
    screen TEXT NOT NULL,
    name TEXT NOT NULL,
    query TEXT NOT NULL,
    filters TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(screen, name)
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_artifacts (
    id TEXT PRIMARY KEY,
    object_kind TEXT NOT NULL,
    object_id TEXT NOT NULL,
    task TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    content TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    UNIQUE(object_kind, object_id, task, provider, model)
);

CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
    object_kind UNINDEXED,
    object_id UNINDEXED,
    title,
    body,
    repository_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Keep the FTS index in sync through explicit statements rather than external
-- content triggers. The store's insert/delete transactions are the single
-- writer, which makes duplicate indexing deterministic and testable.
CREATE TABLE IF NOT EXISTS search_documents (
    object_kind TEXT NOT NULL,
    object_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    repository_id TEXT,
    PRIMARY KEY(object_kind, object_id)
);

CREATE INDEX IF NOT EXISTS idx_prs_repo_state ON pull_requests(repository_id, state, updated_at);
CREATE INDEX IF NOT EXISTS idx_prs_head_sha ON pull_requests(head_sha);
CREATE INDEX IF NOT EXISTS idx_issues_repo_state ON issues(repository_id, state, updated_at);
CREATE INDEX IF NOT EXISTS idx_milestones_repo ON milestones(repository_id, state);
CREATE INDEX IF NOT EXISTS idx_security_repo ON security_alerts(repository_id, state, severity);
CREATE INDEX IF NOT EXISTS idx_commits_repo_time ON commits(repository_id, committed_at);
CREATE INDEX IF NOT EXISTS idx_checks_pr ON checks(pull_request_id, state);
CREATE INDEX IF NOT EXISTS idx_pr_files_path ON pull_request_files(path);
CREATE INDEX IF NOT EXISTS idx_comments_pr ON review_comments(pull_request_id, submitted_at);
CREATE INDEX IF NOT EXISTS idx_knowledge_repo ON knowledge_records(repository_id, kind, observed_at);
CREATE INDEX IF NOT EXISTS idx_notifications_read ON notifications(read_at, created_at);
CREATE INDEX IF NOT EXISTS idx_findings_pr ON review_findings(pull_request_id, severity);
CREATE INDEX IF NOT EXISTS idx_jobs_state ON jobs(state, run_after);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_events(occurred_at);

INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'));
