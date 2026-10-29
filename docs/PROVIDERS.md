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

## Endpoints

| Provider | Request path | Auth |
|---|---|---|
| OpenAI | `POST /v1/responses` | Bearer API key |
| Anthropic | `POST /v1/messages` | `x-api-key` + `anthropic-version` |
| Gemini | `POST /v1beta/models/{model}:generateContent` | API key query parameter |
| OpenAI-compatible | `POST /v1/chat/completions` | Bearer API key |
| Ollama | `POST /api/chat` | local endpoint, no remote key |

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

