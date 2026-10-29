# Release notes

## 0.1.0 — implementation baseline

This is the first functioning GitPilot baseline. It is a development build, not
a production-signed distribution.

### Included

- Native Rust workspace with an egui desktop shell and 15 workspaces.
- Real GitHub REST integration for repository discovery and bounded PR,
  issue, review, check, release and commit synchronization.
- SQLite WAL persistence, FTS5 search, source mirror, migrations v1→v3, recovery,
  backup and explicit local deletion.
- Review engine with deterministic checks, optional structured AI findings,
  evidence and diff-anchor validation, deduplication and finding lifecycle.
- Provider adapters and routing for OpenAI, Anthropic, Gemini,
  OpenAI-compatible endpoints and Ollama.
- Evidence-labelled project brief export, diff-grounded PR/commit explanations,
  project memory, contributor workload, CODEOWNERS suggestions and cross-PR
  overlap signals.
- GitHub Projects v2 GraphQL read sync with typed persistence and an explicit
  unavailable state when permissions/data are missing.
- Guarded create-issue, human review/line-comment and release paths; automation
  engine with dry-run/idempotency; standalone HMAC-verified webhook receiver.
- Local Git inspection plus host-guarded and Docker/Podman command backends.
- Unit, integration, launch and packaging verification documented in
  `TEST_RESULTS.md`.

### Security defaults

- GitHub writes, human review publication, approval/merge and local execution
  are disabled until separately authorized. Automated AI finding publication
  is unavailable.
- External AI transfer is disabled until explicitly granted.
- Credentials use macOS Keychain or Windows Credential Manager. Linux refuses
  plaintext storage until a reviewed Secret Service backend is added.
- Repository content is treated as untrusted evidence; AI output cannot change
  tool permissions.

### Known limitations

- Automated AI finding publication, vector retrieval, signed/notarized
  distribution and VM-grade isolation are intentionally deferred. See
  `FEATURES.md`.
- Docker/Podman runtime verification was blocked by the managed host daemon;
  Linux credential/packaging paths require target-host validation.
- Screenshot artifact is unavailable in this managed session; see
  `SCREENSHOT_UNAVAILABLE.txt`.
- Live GitHub and paid provider requests were not fabricated or executed
  without credentials.

