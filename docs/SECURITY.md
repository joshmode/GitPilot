# Security model and threat model

## Defaults

GitPilot is deny-by-default. Read access, external AI transfer, local execution
and GitHub mutation are separate permission domains. The UI reflects policy, but
the application service re-checks every permission immediately before the side
effect.

Default policy:

- GitHub reads: allowed after credential connection
- External AI context: **denied** until explicitly enabled
- Local commands: **denied** until a structured allow-list entry is enabled
- GitHub writes: **denied**
- Publishing reviews, approvals, merges and releases: **denied**

## Threat model

| Threat | Control |
|---|---|
| Credential theft | OS keychain; tokens never persisted in SQLite or logs; redacted error type; disconnect deletes keychain item |
| Prompt injection in repo text | Repository content is labelled untrusted data; system instructions are separate; tool registry authorization is independent of generated text |
| Malicious repository/build tooling | No execution on import/index; structured commands; allow-list; isolated working directory; timeout and output caps |
| Sandbox escape | Host execution is explicitly not a VM boundary. The optional Docker/Podman backend uses a read-only rootfs and source mount, non-root identity, dropped capabilities, no host home or Docker socket, and resource limits. A container is still not a complete VM security boundary. |
| Arbitrary command execution | Model output cannot contain shell strings that are executed; commands are typed program + argument vectors and validated. PATH entries that are empty, relative or workspace-local are removed. |
| Path traversal / symlinks | Canonical path containment checks; no home-directory mounts; local commands run with a confined working directory |
| Unauthorized GitHub writes | Separate write permission; preview + confirmation; audit; PAT scopes documented. AI findings and automation actions never publish automatically. |
| Data exfiltration | Per-provider transfer consent; local-only mode; secret redaction before prompt assembly |
| Untrusted Markdown | Conservative renderer emits text/layout only; no HTML or external resource loading |
| Oversized/malformed API/diffs | Response byte limits, bounded pagination, hunk/file limits, parse errors surfaced rather than guessed |
| SQLite corruption | WAL, foreign keys, transactional migrations, online backup API, integrity check |
| Retry storms / rate limits | Classified errors, capped exp backoff, `Retry-After`, request deduplication, bounded concurrency |

## Residual risks

- A token with write privileges can perform writes if a user explicitly enables
  writes and confirms an operation. GitPilot cannot provide stronger authority
  than the GitHub token.
- Native process execution on the host is not equivalent to a container or VM.
  Commands are disabled by default. The host implementation provides environment
  scrubbing and timeout/output limits, but the OS remains part of the trust
  boundary. Docker/Podman mode is stricter but is not a VM guarantee either.
- GitHub Enterprise Server certificates and endpoints need deployment-specific
  validation. Only HTTPS endpoints are accepted.
- AI providers receive whatever evidence is included in the reviewed request.
  Users can inspect context before enabling transfer.
- SQLite is built with SQLx's bundled SQLite (`libsqlite3-sys` bundled feature),
  so the app does not depend on a host `libsqlite3` version. FTS5 is available
  in the bundled build; verify the packaged binary on each target release.
- The release/app bundle was built unsigned; do not represent it as a trusted
  distribution artifact.
- Security endpoints are permission-sensitive. A 403/404 is represented as
  `Unavailable`, not as “no alerts.” A partial endpoint failure does not erase
  previously cached alerts.
- The command runner rejects path-like executables, privileged execution,
  excessive argument lists and NUL bytes. It scrubs the environment, but the
  child process still runs on the host; this is not a VM guarantee.
- On macOS, network-denied command policies are enforced with `sandbox-exec`.
  Docker/Podman uses `--network none` unless the separate policy allows network.
  The managed test environment could not access the Docker daemon, so live
  container isolation was not claimed as verified.

## Secrets and logging

Provider keys and GitHub tokens use the credential store and are excluded from
`Debug`. Request tracing records method, host, status and duration but not
authorization headers or bodies. Findings and audit metadata are redacted with
conservative token/private-key patterns.

