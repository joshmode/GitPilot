# Implementation tracker

Updated continuously. `DONE` means the user-facing path and service behavior are
implemented and tested; `PARTIAL` is documented in FEATURES.md.

## Milestone 0 — assessment
- [x] Empty repository confirmed; no prior code to preserve.
- [x] Toolchain, platform and offline dependency baseline inspected.
- [x] Product requirements, architecture, security notes and ADRs created.

## Milestone 1 — foundation
- [x] Cargo workspace and app shell
- [x] Design tokens, light/dark themes and keyboard navigation
- [x] SQLite migrations, FTS5, backup and data deletion
- [x] Structured error boundary and tracing
- [x] Demo mode with labelled fixtures
- [x] Persisted background jobs with cancellation and retry
- [x] Native macOS UserNotifications delivery adapter with bundle detection and fail-closed fallback

## Milestone 2 — GitHub
- [x] macOS Keychain and Windows Credential Manager adapters; unsupported Linux backend refuses plaintext
- [x] REST client: auth, pagination, rate limits, conditional requests
- [x] Repository discovery/selection and multi-account storage
- [x] PR, issue, commit, review, check and release sync
- [x] Incremental PR sync and FTS search
- [x] GitHub Projects v2 GraphQL read sync, typed persistence and explicit unavailable UI

## Milestone 3 — intelligence
- [x] Dashboard metrics with explicit missing-data states
- [x] PR cockpit, filters, detail and diff viewer
- [x] Commit explorer and compare view
- [x] Evidence-labelled project brief
- [x] Project brief export with evidence categories and unavailable-data disclosure
- [x] Repository health and activity

## Milestone 4 — providers
- [x] Unified provider interface and capability negotiation
- [x] OpenAI, Anthropic, Gemini, compatible and Ollama adapters
- [x] Routing, budgets, fallback disclosure and usage accounting
- [x] Structured generation with deterministic validation

## Milestone 5 — review
- [x] Review pipeline with deterministic checks and AI findings
- [x] Evidence and diff-location validation
- [x] Deduplication, lifecycle and persisted runs
- [x] Review policies and GUI cockpit
- [x] Guarded human summary/line-comment and selected-finding publishing in the PR cockpit
- [x] Reviewer suggestions from CODEOWNERS/workload evidence; never auto-assigned

## Milestone 6 — collaboration/memory
- [x] Contributor workload and review bottlenecks
- [x] Cross-PR file/symbol overlap signals
- [x] Knowledge records with provenance and FTS retrieval
- [x] Evidence-backed workspace Q&A
- [x] Vector/hybrid retrieval (opt-in; FTS remains the default)

## Milestone 7 — automation/local
- [x] Workflow definitions, deterministic conditions/actions, dry run
- [x] Persisted queue, idempotency and run history
- [x] Local Git read workspace and comparison
- [x] Guarded structured command sandbox
- [x] Event polling fallback and standalone HMAC-verified, deduplicated webhook receiver
- [x] Host process guard plus structured Docker/Podman backend with read-only source and resource limits
- [ ] Native container execution verification (runtime daemon unavailable in the managed test environment)

## Milestone 8 — hardening
- [x] Unit/integration tests and fixture-based GitHub tests
- [x] Clippy/fmt gate
- [x] macOS bundle script and packaging docs
- [ ] Screenshot artifact (attempted; display unavailable; command documented)
- [x] Feature matrix and known limitations

## Post-audit corrections
- [x] Full bounded GitHub pagination and authoritative PR detail retrieval
- [x] My PRs / needs-my-review user scoping and SQL-backed view filters
- [x] Merged/closed windows and milestone snapshots
- [x] Code-scanning, Dependabot and secret-scanning ingestion
- [x] Automatic private-review execution with one-run-per-head protection
- [x] Durable redacted file logging and startup data-directory diagnostics
- [x] Local command executable/privilege/argument validation
- [x] Repository scope switching, saved-view recall and command-palette recents
- [x] On-demand commit diff viewer and generated-file persistence
- [x] Commit explanations receive bounded diff hunks and disclose unavailable evidence
- [x] Cross-platform notification, sandbox, project, webhook and credential-path tests
- [x] Provider streaming, explanation depth selection, branch overview and saved-view filter round trips

