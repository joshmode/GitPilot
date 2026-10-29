# ADR 0002: REST-first GitHub integration

Status: accepted

Use REST for synchronization and mutations. GraphQL is used only for GitHub
Projects v2, where the REST API does not expose the project/item schema, and is
kept behind a typed client boundary. REST provides clear ETag/conditional
request behavior, pagination, rate-limit headers and granular error statuses.
Projects v2 requires a token with project read access; the UI reports missing
data rather than inventing progress.
