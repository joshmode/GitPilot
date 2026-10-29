# Release notes

## 0.1.0 — implementation baseline

This is the first functioning GitPilot baseline. It is a development build, not
a production-signed distribution.

### Included

- Native Rust workspace with an egui desktop shell and 15 workspaces.
- Real GitHub REST integration for repository discovery and bounded PR,
  issue, review, check, release and commit synchronization.
- SQLite WAL persistence, FTS5 search, source mirror, migrations, recovery,
  backup and explicit local deletion.
- Review engine with deterministic checks, optional structured AI findings,
  evidence and diff-anchor validation, deduplication and finding lifecycle.
- Provider adapters and routing for OpenAI, Anthropic, Gemini,
  OpenAI-compatible endpoints and Ollama.
- Evidence-labelled project brief, PR/commit explanations, project memory,
  contributor workload and cross-PR overlap signals.
- Guarded create-issue path, automation engine with dry-run/idempotency and a
  local Git inspection workspace.
- Unit, integration, launch and packaging verification documented in
  `TEST_RESULTS.md`.

### Security defaults

- GitHub writes, review publication, approval/merge and local execution are
  disabled until separately authorized.
- External AI transfer is disabled until explicitly granted.
- Credentials use the macOS Keychain. Other platforms refuse plaintext storage
  until a native credential adapter is implemented.
- Repository content is treated as untrusted evidence; AI output cannot change
  tool permissions.

### Known limitations

- Review publication, Projects v2, webhook receiver, vector retrieval, native
  OS notifications, signed/notarized distribution and container/VM isolation
  are deferred. See `FEATURES.md`.
- Screenshot artifact is unavailable in this managed session; see
  `SCREENSHOT_UNAVAILABLE.txt`.
- Live GitHub and paid provider requests were not fabricated or executed
  without credentials.

