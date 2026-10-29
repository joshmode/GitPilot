# Provider integration guide

## Contracts

`ProviderAdapter` in `gitpilot-core/src/ai.rs` separates protocol adapters from
routing, review and UI. Each adapter reports capabilities and supports:

- single-shot generation
- cancellation through `CancellationToken`
- HTTP timeout and classified errors
- health check
- model discovery where the provider exposes it

Structured output is requested per provider rather than assuming every endpoint
implements one generic JSON mode. The review engine validates returned JSON and
only persists a finding whose file and line are present in the cached diff.

Streaming is implemented for the documented OpenAI Responses,
Anthropic Messages, Gemini streaming, OpenAI-compatible chat and Ollama chat
event formats. The workspace streams text deltas and records the returned token
usage; the mock provider explicitly reports that it does not stream instead of
pretending to support it.

## Endpoints

| Provider | Request path | Auth |
|---|---|---|
| OpenAI | `POST /v1/responses` | Bearer API key |
| Anthropic | `POST /v1/messages` | `x-api-key` + `anthropic-version` |
| Gemini | `POST /v1beta/models/{model}:generateContent` | API key query parameter |
| OpenAI-compatible | `POST /v1/chat/completions` | Bearer API key |
| Ollama | `POST /api/chat` | local endpoint, no remote key |

Model discovery is limited to providers that expose a compatible catalog:
OpenAI/OpenAI-compatible `/v1/models`, Gemini's model-list endpoint, and Ollama
`/api/tags`. Anthropic models must be entered explicitly because its list
endpoint is not part of the request contract used by this client.

Remote endpoints must use HTTPS. The only permitted HTTP endpoint is local
Ollama at a loopback address.

## Routing and budgets

`routing_policies` maps task names such as `review.review`,
`pr.explain`, `commit.explain` and `workspace.ask` to a primary provider,
optional fallback, input-token ceiling and per-task cost ceiling.

A request is rejected if the estimated token count or estimated response cost
exceeds the policy. A fallback is recorded (`fallback_used`) and displayed in
usage. The app never silently substitutes a weaker model without that metadata.

Prices are user-configured estimates, not a live billing source. The UI labels
cost as estimated.

## Optional embeddings

Hybrid retrieval is disabled by default. Configure an embedding model on a
provider, enable it in **Settings → Notifications/Knowledge** or the AI
workspace, and index a repository explicitly. OpenAI, OpenAI-compatible,
Gemini and Ollama embedding protocols are supported per provider; Anthropic is
not an embedding provider.

Embeddings are stored locally as bounded vectors with a content hash. Changing
a knowledge record invalidates its vector. FTS5 remains the default and no
embedding provider is contacted unless the user enables the feature and grants
the external-transfer policy for a remote provider.

## Usage ledger

The `provider_usage` table records provider, model, task, input/output tokens,
latency, retry/fallback status, cancellation and estimated cost. API keys are
never included in a record.

## Adding a provider

1. Add a `ProviderKind` with explicit capabilities.
2. Implement the request and response shape in a dedicated adapter method.
3. Map the provider's token usage fields.
4. Add a mock/fixture test for auth failure, malformed output and cancellation.
5. Add the provider to `default_provider_configs` only if a safe default model
   and price basis are documented.
6. Do not route repository content to it until the privacy policy explicitly
   permits external transfer.

