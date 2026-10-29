# Feature completion matrix

Last verified: 2026-10-29. This inventory is intentionally conservative. A
feature is marked **Implemented** only where the visible UI path calls a real
service implementation.

| Area | Status | Evidence and limits |
|---|---|---|
| Native Rust/egui GUI | Implemented | `cargo run -p gitpilot-app`; 15 workspaces, dark/light tokens, resizable sidebar |
| Demo mode | Implemented | Seeded account, 3 repositories, 6 PRs, issues, releases, checks, diffs, knowledge and notifications; every origin is labelled `Demo data` |
| SQLite persistence | Implemented | WAL, foreign keys, versioned migrations v1→v3, FTS5, search source mirror, indexed list queries |
| Background jobs | Implemented | Persisted queue, dedupe, recovery, retry, cancellation, bounded dispatcher and UI inspector; sync/review jobs dispatch through registered handlers |
| GitHub repository discovery | Implemented | REST `/user/repos`, pagination, account-scoped persistence |
| GitHub PR/issue/release sync | Implemented | Full bounded pagination, ETag conditional requests, PR details/checks/reviews/files, merged/closed windows, milestones, releases |
| GitHub commits sync | Implemented | Bounded default-branch history sync (up to 300 commits), parent/author/verification metadata and PR linkage; no full-history mirror |
| GitHub reviews | Implemented | Read comments/decisions with current-head logic; guarded human summary review and validated line comments |
| Guarded GitHub write test | Implemented | Create-issue path requires write permission plus a typed confirmation; demo repositories cannot be mutated |
| GitHub Projects v2 | Implemented | Typed GraphQL repository-owner query, pagination, status-field normalization, project/item persistence and explicit unavailable state; live verification requires a token with Projects v2 access |
| GitHub OAuth | Implemented | Public-client device flow with explicit client ID; PAT and OS keychain remain supported. GitHub App private-key flow still requires a backend |
| Multi-account storage | Implemented | Separate account IDs, hosts and OS credential entries |
| Dashboard | Implemented | Database-derived PR/review/CI/stale/issue metrics, saved snapshot, repository health, explicit missing-data states |
| Project brief | Implemented | Deterministic, evidence-labelled brief; generated AI synthesis optional |
| PR cockpit | Implemented | Views, search, pagination, metadata, checks, discussion, findings, readiness and explanation entry |
| Diff viewer | Implemented | Unified/side-by-side, strict hunk parsing, real line anchors, binary and generated-file handling |
| Commit explorer | Implemented | Persisted commits, metadata, PR linkage, diff-grounded explanations with uncertainty, review focus and on-demand GitHub commit-file diff viewer |
| Cross-PR overlap | Implemented | Deterministic shared-file signals; intentionally not labelled as a conflict |
| Contributor workspace | Implemented | Assignment/review workload, bottleneck signals, CODEOWNERS-backed reviewer suggestions and configurable individual counters; never auto-assigned or used as productivity ranking |
| Provider adapters | Implemented | Independent OpenAI Responses, Anthropic Messages, Gemini, OpenAI-compatible and Ollama protocols, plus mock |
| Provider routing/budgets | Implemented | Persisted policies, cost estimates, fallback disclosure fields, usage ledger |
| Model discovery | Implemented | OpenAI/OpenAI-compatible `/v1/models`, Gemini model-list and Ollama `/api/tags`; Anthropic uses documented explicit model entry |
| AI code review | Implemented | Deterministic checks, optional structured AI review, confidence/evidence validation, dedupe, lifecycle, persisted runs, file exclusions and review-policy validation |
| Review publishing | Implemented | Human comment/changes-requested/approval submission and line comments require explicit permission plus confirmation; AI findings stay private |
| Knowledge memory | Implemented | FTS5 retrieval, provenance, explicit-fact flag, local documentation ingestion |
| Vector/hybrid retrieval | Deferred | Embeddings optional by design; FTS5 is functional without external services |
| NL investigation | Implemented | Local evidence answer or provider synthesis; empty retrieval is not negative evidence |
| Automation engine | Implemented | Typed triggers/conditions/actions, validated GUI builder, dry run, approval gates, idempotency, run history, scheduled tick and webhook event ingestion; unattended scheduling can use an external scheduler |
| Webhook receiver | Implemented | Separate `gitpilot-webhook` binary verifies HMAC-SHA256 over raw bytes, caps payloads, deduplicates delivery IDs, persists verified events and routes typed triggers; polling remains fallback and the desktop app opens no public listener |
| Local Git | Implemented | Connect working tree, status, branch list, tags, log, diff/compare, GitHub compare and indexed code search through argument-safe Git commands |
| Safe execution | Implemented with host-runtime limitation | Structured command vectors, allow-list, timeout/cancellation, sanitized PATH, macOS `sandbox-exec` network denial, plus Docker/Podman backend with non-root user, dropped capabilities, read-only root/source, no host home or Docker socket, bounded CPU/memory/PIDs and network disabled by default; runtime execution was not verifiable because the managed daemon was inaccessible |
| Search/palette | Implemented | FTS5 global search, command palette, recent items, saved-view storage API, keyboard reference |
| Notifications | Implemented | In-app centre, read/snooze, dedupe keys, quiet hours, per-repository policy and native macOS UserNotifications delivery gated by packaged-app detection; other platforms report unsupported rather than faking delivery |
| Release workspace | Implemented | Release history, evidence-backed readiness, blockers, dependency/breaking-change/migration signals, draft notes and guarded draft/publish creation |
| Security signals | Implemented | Code-scanning, Dependabot and secret-scanning ingestion with explicit not-checked/unavailable/no-alerts/findings states |
| Audit log | Implemented | Mutations, review runs, automation runs, command execution, permission/provider changes; TSV export from Settings |
| Backup/deletion | Implemented | SQLite `VACUUM INTO`, integrity check, explicit typed deletion confirmation |
| macOS packaging | Implemented | `scripts/package-macos.sh` creates `.app`; signing/notarization documented but not performed without certificates |
| Windows/Linux packaging | Partial | Windows Credential Manager adapter compiles behind its target; Linux refuses plaintext until a reviewed Secret Service backend is added. Packaging/signing still require target-host validation |

## Explicit limitations

1. **No real credentials are included.** Live GitHub and provider operations
   require the user’s accounts and are error-tested with fixtures/mocks.
2. **AI finding publication is intentionally disabled.** The GUI supports
   human-authored summary reviews and validated line comments with separate
   permissions and confirmation. No AI finding is published automatically.
3. **Host execution is not a VM.** The command runner has allow-lists,
   timeouts and environment scrubbing, but the OS process is not a complete
   isolation boundary.
4. **Projects v2 and webhooks need live credentials/deployment.** Projects v2
   has a fixture-tested GraphQL path but no live token verification here.
   Webhooks require deploying the separate receiver behind TLS; the desktop app
   does not claim to be a public listener.
5. **Visual QA is limited.** The native process launched successfully and
   initialized demo SQLite; this managed session could not reliably expose a
   screenshot through the renderer or display capture. Use
   `scripts/capture-screenshot.sh` on a desktop session.
6. **Approval/merge remains disabled by default.** Human summary reviews, line
   comments, issue creation and releases each require separate permissions and
   confirmation; branch protection is never bypassed.
7. **Important host validation is still environment-specific.** The release
   binary and app bundle were built on macOS arm64, but launch behavior under
   LaunchServices and code signing could not be fully verified in this
   session.

