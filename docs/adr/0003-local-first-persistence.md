# ADR 0003: SQLite WAL and FTS5

Status: accepted

Persist normalized metadata in SQLite with WAL and foreign keys. Use FTS5 for
search. Credentials and large Git objects are not stored. Migrations are versioned
and applied transactionally at startup.
