# Verification results

Run date: 2026-10-29

## Executed

```text
cargo check --workspace --offline
  -> Finished successfully

cargo test --workspace --offline
  -> 37 core unit tests passed
  -> 33 service/database integration tests passed (current suite)
  -> webhook binary and app unit targets compiled and ran
  -> 0 app unit tests (GUI smoke tested separately)

/opt/homebrew/Cellar/rust/1.99.1/bin/cargo-clippy --offline --workspace --all-targets -- -D warnings
  -> Finished successfully

/opt/homebrew/Cellar/rust/1.99.1/bin/cargo-fmt --all -- --check
  -> exit 0
```

The direct Homebrew toolchain invocations are used because the managed sandbox
prevents `rustup` from creating its temp files. This is an environment
constraint, not a suppressed lint.

## Native launch smoke test

`GITPILOT_DATA_DIR=/tmp/gitpilot-optional-smoke target/release/gitpilot` remained alive
after startup. It created a WAL-mode SQLite database and logs. SQLite reported:

```text
3 repositories
6 pull requests
1 CODEOWNERS knowledge fixture
0 embeddings before explicit indexing
4 providers with optional embedding models
PRAGMA integrity_check -> ok
```

The window process emitted only macOS service diagnostics
(`NSWorkspaceNotificationCenter`/`hiservices-xpcservice`), not an application
panic. The environment did not permit taking a screenshot.

## Additional audit verification

The review pass also added regression coverage for:

- SQL-backed PR view filtering and pagination/count agreement
- user-scoped “My PRs” and “Needs my review”
- deterministic and AI review idempotency and policy file exclusions
- security-alert persistence and clear/replace behavior
- duplicate job suppression
- symlink-safe local documentation ingestion
- command executable/privilege rejection
- enterprise-host validation and base64 content bounds
- automatic private-review execution once per PR head
- persisted job dispatcher startup and runtime initialization

Latest workspace test totals: **37 unit tests + 33 integration tests**, all
passing. Strict Clippy and `cargo fmt --check` also pass.

Final release-binary smoke test also initialized schema migration version 4,
3 demo repositories, 6 pull requests, 0 jobs and reported
`PRAGMA integrity_check = ok`.

Post-audit additions include a live job dispatcher, migration v1→v2 coverage,
security-alert ingestion, release reports, commit diff retrieval, guarded
GitHub write/review boundaries, Projects v2, webhook verification, native
notification fallback and source-link navigation fixes. The latest workspace
run is **37 unit + 33 integration tests**, all passing, with fmt
and strict Clippy clean.

## Not executed

- Live GitHub API calls: no user credential was provided. Transport, pagination,
  authentication-error and rate-limit paths are covered by focused unit tests;
  live content is not claimed as verified.
- Live provider calls: no paid/external credentials were provided. The
  deterministic provider and IMDS protocol shapes are tested.
- Notarization/code signing: no Apple Developer identity was available.
- Reliable GUI screenshot capture: the managed session exposed no usable
  display to the capture path; this is recorded explicitly rather than replaced
  with a mockup.
- LaunchServices app opening was unavailable in this managed macOS session, so
  the bundle was validated structurally and its executable was launched
  directly. This is not equivalent to a signed, notarized user install test.
- Live Docker/Podman execution: the managed session could not access the
  Docker daemon. The backend's structured policy is unit-tested, but runtime
  container isolation is not claimed as verified here.
- Native macOS notification delivery from a packaged app: the adapter compiled
  and its bare-binary fallback is tested, but OS authorization/banner delivery
  requires a signed desktop session.
- Live Projects v2 and webhook requests: normalized GraphQL fixtures and
  signature/dedupe tests pass; no live token or deployed receiver was used.

## Regression notes

- A first integration run caught a review-run foreign-key/idempotency defect.
  The fix preserves the first run ID per `(pull_request, head_sha)` and updates
  findings against it.
- A first run also caught cost-column decoding and FTS integrity issues.
  `COALESCE` is now cast to REAL; FTS has a relational source mirror and an
  explicit rebuild/recovery path.

