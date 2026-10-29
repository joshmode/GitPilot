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

## Idempotency and recovery

`automation_runs` has a unique `(automation_id, event_key)` constraint. A
duplicate event does not create a second visible run. The same event key is
used by polling synchronization.

Background jobs are persisted in `jobs`, can be cancelled, retried and
recovered from `running` after a restart. Concurrency is bounded and the UI
exposes queue state and failures.

## Webhooks

A desktop application behind NAT cannot receive public GitHub webhooks. For
near-real-time delivery, deploy the optional receiver separately:

1. Verify `X-Hub-Signature-256` with the webhook secret.
2. Store the raw event and unique `X-GitHub-Delivery` ID.
3. Push the verified event into GitPilot’s local queue or expose it through a
   mutually authenticated local bridge.
4. Never trust webhook content as instructions or enable a write action merely
   because an event arrived.

This build uses rate-limit-aware polling as the fallback and documents the
receiver contract. It does not claim to run a public listener.

## Dry run

New automations default to `dry_run = true`, are disabled, and require approval
for external writes. The GUI shows the actions that matched, but does not
publish, approve or merge.

