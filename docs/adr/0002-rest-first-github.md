# ADR 0002: REST-first GitHub integration

Status: accepted

Use REST for synchronization and mutations. GraphQL remains behind a typed
boundary for future Projects v2 support. REST provides clear ETag/conditional
request behavior, pagination, rate-limit headers and granular error statuses.
GitHub Projects support is partial until an installation-scoped GraphQL token is
available.
