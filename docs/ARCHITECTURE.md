# Architecture

## Shape

GitPilot uses a two-crate workspace rather than a swarm of microcrates:

- `gitpilot-core` owns typed domain models, SQLite persistence, GitHub REST
  integration, AI adapters/routing, deterministic intelligence, diff parsing,
  search, background jobs, local Git inspection and guarded command execution.
- `gitpilot-app` owns the native egui shell, design system, screens and UI state.
  It calls application services through `AppService`; widgets never issue
  arbitrary HTTP requests or mutate SQLite themselves.

This keeps the dependency graph small while preserving explicit boundaries.

## Runtime

The main thread runs the native event loop. A Tokio multi-thread runtime owns
HTTP, synchronization, AI, indexing and local process work. UI events are
dispatched through a bounded channel; results return as typed `UiEvent`s. The UI
always remains renderable during network activity.

`JobRunner` provides bounded concurrency, cancellation, retry, persisted status
and recovery of interrupted work after restart. Long operations receive a
`CancellationToken` and a deadline.

## Data flow

1. GitHub REST responses become wire models, then normalized domain records.
2. Repository, PR, issue, check, review, commit, milestone and security-alert
   records are upserted by stable external IDs.
3. FTS5 indexing is kept synchronized by the store's explicit insert/delete
   transactions, using a relational `search_documents` source mirror.
4. Intelligence derives metrics from stored records with an explicit observation
   time. Missing data remains `Unavailable`, never zero.
5. AI requests receive a scoped evidence packet. The response is parsed and
   attached to a persisted run with provider, model, tokens, latency and cost.
6. Every external mutation checks a `PermissionPolicy` at the service boundary
   and records an audit event after completion or failure.
7. Repository documentation and PR/issue descriptions are indexed with
   provenance after sync; local documentation ingestion rejects symlinks and
   stays inside the selected working tree.
8. Private review policy execution is idempotent per PR head SHA. Automatic
   reviews remain local unless publication is separately authorized.

## Key decisions

- **REST first, GraphQL for Projects v2.** REST covers discovery, PRs, commits,
  checks, issues, releases and security alerts with predictable pagination.
  GraphQL is limited to the Projects v2 schema surface and kept behind a typed
  client boundary.
- **FTS5 before vectors.** Full-text retrieval is local, deterministic, cheap and
  works without embeddings. The schema reserves retrieval provenance and allows
  a vector table/provider later.
- **One rich SQLite schema.** JSON is limited to flexible metadata (raw labels,
  findings evidence and automation definitions). Core identity and relationships
  are relational with uniqueness constraints.
- **Native credential stores.** Tokens are addressed by account ID and stored
  outside the database in macOS Keychain or Windows Credential Manager.
  Unsupported Linux hosts use an explicit refusing adapter rather than silently
  writing plaintext.
- **No full repository mirror.** GitPilot caches metadata and bounded diffs. Full
  history and blobs stay in Git. Local repositories are inspected through
  non-interactive Git commands with structured arguments.

See ADR files in `docs/adr/`.

