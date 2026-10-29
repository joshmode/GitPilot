# Automations and event ingestion

## Supported event model

The workflow engine accepts a typed `Event`:

- PR opened, synchronized or ready for review
- review submitted
- CI failed or succeeded
- issue assigned
- label added
- release published
- scheduled check or manual trigger

Conditions are deterministic (`repository:`, `label:`, `payload.key=value`).
Actions are typed (`run_ai_review`, `create_notification`, `create_issue`,
`add_label`, review publication/approval actions, patch preparation). A
write action checks the automation’s permission policy at execution time.
Actions that require approval remain prepared rather than executed.

The Automations screen includes a validated builder for the trigger, condition
and action lists. Saving a write action requires matching application and
workflow grants; the default policy rejects it rather than creating a workflow
that can never execute.

## Idempotency and recovery

`automation_runs` has a unique `(automation_id, event_key)` constraint. A
duplicate event does not create a second visible run. The same event key is
used by polling synchronization.

Scheduled checks are opt-in. “Run scheduled checks now” is the deterministic
desktop tick; while the app runs and the permission is enabled, the same tick
also runs every 15 minutes. The event key includes the UTC hour so repeated
ticks cannot create multiple runs for the same automation in that hour. A
deployment can invoke the same tick from an external scheduler if it needs
unattended execution while the desktop app is closed.

Background jobs are persisted in `jobs`, can be cancelled, retried and
recovered from `running` after a restart. Concurrency is bounded and the UI
exposes queue state and failures.

## Webhooks

A desktop application behind NAT cannot receive public GitHub webhooks. For
near-real-time delivery, deploy the optional `gitpilot-webhook` binary
separately:

```bash
cargo build --release -p gitpilot-app --bin gitpilot-webhook
GITPILOT_WEBHOOK_SECRET_FILE=/secure/path/secret.txt \
GITPILOT_WEBHOOK_BIND=127.0.0.1:8787 \
GITPILOT_DATA_DIR=/var/lib/gitpilot \
target/release/gitpilot-webhook
```

Point GitHub's webhook at `POST /github/webhook`. Put it behind TLS and a
network policy; do not expose the desktop app itself. The receiver:

1. Verifies `X-Hub-Signature-256` with constant-time HMAC-SHA256 over the raw
   body before parsing.
2. Rejects bodies above 2 MiB and malformed delivery identifiers.
3. Stores the verified event with a unique `X-GitHub-Delivery` ID, which makes
   replay and duplicate delivery idempotent.
4. Exposes unprocessed deliveries to the desktop's persisted job queue. The
   “Process verified webhook deliveries” action enqueues local workflow
   evaluation only; it never performs an external write.

The secret must be at least 32 bytes. Keep it in a secret manager or a
root-owned file readable only by the receiver process, not in Git or a public
configuration file. No repository content from a webhook can change tool
permissions or trigger a write action by itself.

This build also keeps rate-limit-aware polling as the fallback. It does not
claim to run a public listener inside the desktop app.

## Dry run

New automations default to `dry_run = true`, are disabled, and require approval
for external writes. The GUI shows the actions that matched, but does not
publish, approve or merge.

