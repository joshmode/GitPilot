# Feature completion matrix

Last verified: 2026-10-29. This inventory is intentionally conservative. A
feature is marked **Implemented** only where the visible UI path calls a real
service implementation.

| Area | Status | Evidence and limits |
|---|---|---|
| Native Rust/egui GUI | Implemented | `cargo run -p gitpilot-app`; 15 workspaces, dark/light tokens, resizable sidebar |
| Demo mode | Implemented | Seeded account, 3 repositories, 6 PRs, issues, releases, checks, diffs, knowledge and notifications; every origin is labelled `Demo data` |
| SQLite persistence | Implemented | WAL, foreign keys, versioned migrations including v1→v2 upgrade, FTS5, search source mirror, indexed list queries |
| Background jobs | Implemented | Persisted queue, dedupe, recovery, retry, cancellation, bounded dispatcher and UI inspector; sync/review jobs dispatch through registered handlers |
| GitHub repository discovery | Implemented | REST `/user/repos`, pagination, account-scoped persistence |
| GitHub PR/issue/release sync | Implemented | Full bounded pagination, ETag conditional requests, PR details/checks/reviews/files, merged/closed windows, milestones, releases |
| GitHub commits sync | Implemented | Bounded default-branch history sync (up to 300 commits), parent/author/verification metadata and PR linkage; no full-history mirror |
| GitHub reviews | Implemented | Read comments/decisions with current-head logic; guarded human summary review and validated line comments |
| Guarded GitHub write test | Implemented | Create-issue path requires write permission plus a typed confirmation; demo repositories cannot be mutated |
| GitHub Projects v2 | Deferred | GraphQL/installation architecture documented; no Projects API implementation in this build |
| GitHub OAuth | Implemented | Public-client device flow with explicit client ID; PAT and OS keychain remain supported. GitHub App private-key flow still requires a backend |
| Multi-account storage | Implemented | Separate account IDs, hosts and keychain entries |
| Dashboard | Implemented | Database-derived PR/review/CI/stale/issue metrics, saved snapshot, repository health, explicit missing-data states |
| Project brief | Implemented | Deterministic, evidence-labelled brief; generated AI synthesis optional |
| PR cockpit | Implemented | Views, search, pagination, metadata, checks, discussion, findings, readiness and explanation entry |
| Diff viewer | Implemented | Unified/side-by-side, strict hunk parsing, real line anchors, binary and generated-file handling |
| Commit explorer | Implemented | Persisted commits, metadata, PR linkage, AI explanation and on-demand GitHub commit-file diff viewer |
| Cross-PR overlap | Implemented | Deterministic shared-file signals; intentionally not labelled as a conflict |
| Contributor workspace | Implemented | Assignment/review workload and bottleneck signals; no individual productivity ranking |
| Provider adapters | Implemented | Independent OpenAI Responses, Anthropic Messages, Gemini, OpenAI-compatible and Ollama protocols, plus mock |
| Provider routing/budgets | Implemented | Persisted policies, cost estimates, fallback disclosure fields, usage ledger |
| Model discovery | Implemented | OpenAI/OpenAI-compatible/Ollama discovery; Anthropic/Gemini use documented model entry because their endpoints do not share the same discovery contract |
| AI code review | Implemented | Deterministic checks, optional structured AI review, confidence/evidence validation, dedupe, lifecycle, persisted runs, file exclusions and review-policy validation |
| Review publishing | Implemented | Human comment/changes-requested/approval submission and line comments require explicit permission plus confirmation; AI findings stay private |
| Knowledge memory | Implemented | FTS5 retrieval, provenance, explicit-fact flag, local documentation ingestion |
| Vector/hybrid retrieval | Deferred | Embeddings optional by design; FTS5 is functional without external services |
| NL investigation | Implemented | Local evidence answer or provider synthesis; empty retrieval is not negative evidence |
| Automation engine | Implemented | Typed triggers/conditions/actions, dry run, idempotency, scoped + global permission gate, run history; scheduled execution still requires an external scheduler |
| Webhook receiver | Deferred | Polling fallback and deployment architecture documented; desktop app does not pretend to receive public webhooks |
| Local Git | Implemented | Connect working tree, status, branch list, tags, log, diff/compare, GitHub compare and indexed code search through argument-safe Git commands |
| Safe execution | Partial | Structured command vectors, allow-list, timeout/cancellation, environment scrubbing, macOS `sandbox-exec` network denial; container/VM isolation is not bundled |
| Search/palette | Implemented | FTS5 global search, command palette, recent items, saved-view storage API, keyboard reference |
| Notifications | Implemented | In-app centre, read/snooze, dedupe keys, quiet hours, per-repository policy; native OS notifications deferred |
| Release workspace | Implemented | Release history, evidence-backed readiness, blockers, dependency/breaking-change/migration signals, draft notes and guarded draft/publish creation |
| Security signals | Implemented | Code-scanning, Dependabot and secret-scanning ingestion with explicit not-checked/unavailable/no-alerts/findings states |
| Audit log | Implemented | Mutations, review runs, automation runs, command execution, permission/provider changes; TSV export from Settings |
| Backup/deletion | Implemented | SQLite `VACUUM INTO`, integrity check, explicit typed deletion confirmation |
| macOS packaging | Implemented | `scripts/package-macos.sh` creates `.app`; signing/notarization documented but not performed without certificates |
| Windows/Linux packaging | Partial | Clean abstraction path documented; native keychain adapter is macOS-only and refuses plaintext elsewhere |

## Explicit limitations

1. **No real credentials are included.** Live GitHub and provider operations
   require the user’s accounts and are error-tested with fixtures/mocks.
2. **AI review publication is disabled.** The service has a guarded human
   review-submission path, but it is not exposed in the GUI and no AI finding is
   published automatically.
3. **Host execution is not a VM.** The command runner has allow-lists,
   timeouts and environment scrubbing, but the OS process is not a complete
   isolation boundary.
4. **GitHub Projects and webhooks need deployment-grade plumbing.** The UI
   labels these as unavailable/deferred rather than showing fake progress.
5. **Visual QA is limited.** The native process launched successfully and
   initialized demo SQLite; this managed session could not reliably expose a
   screenshot through the renderer or display capture. Use
   `scripts/capture-screenshot.sh` on a desktop session.
6. **Review publication and approval/merge remain disabled.** The create-issue
   path is the only live GitHub mutation currently exposed; it requires both
   the write permission and a confirmation.
7. **Important host validation is still environment-specific.** The release
   binary and app bundle were built on macOS arm64, but launch behavior under
   LaunchServices and code signing could not be fully verified in this
   session.

