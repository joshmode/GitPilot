# Setup and first run

## Development

```bash
rustup toolchain install stable
cargo run -p gitpilot-app
```

The app starts in Demo mode. It creates a SQLite database in the OS application
data directory and never invents a live GitHub connection.

Set an isolated profile while developing:

```bash
GITPILOT_DATA_DIR=/tmp/gitpilot-dev cargo run -p gitpilot-app
GITPILOT_DEMO=0 GITPILOT_DATA_DIR=/tmp/gitpilot-live cargo run -p gitpilot-app
```

## GitHub credential

1. Open **Settings → GitHub → Connect account**.
2. Use a fine-grained PAT scoped to the repositories you need.
3. Grant read access to:
   - Metadata
   - Contents
   - Issues
   - Pull requests
   - Checks
   - Code scanning alerts (if your token is permitted to read them)
   - Dependabot alerts (if your token is permitted to read them)
   - Secret scanning alerts (if your token is permitted to read them)
   - Administration read (optional; required by GitHub for branch-protection details)
4. For GitHub Enterprise Server, enter the host (for example
   `github.example.com`). GitPilot uses `https://HOST/api/v3`.
5. Connect, then open **Repositories → Discover from GitHub**.
6. Use **Sync live repositories** to fetch PRs, issues, checks, comments and
   releases.

Security endpoints can return `403` or `404` when the token does not have the
relevant permission. GitPilot surfaces that as **Unavailable**, not “no
alerts.”

The token is stored under service `com.gitpilot.desktop` in the macOS Keychain
or Windows Credential Manager. It is not written to SQLite, logs, exported
reports or prompts. Linux builds currently refuse credential persistence until
a reviewed Secret Service backend is added rather than silently writing
plaintext.

### GitHub App / OAuth

Fine-grained PATs and the public-client OAuth device flow are supported. For an
organizational deployment, prefer a small backend that holds the GitHub App
private key, completes the installation callback and mints short-lived
installation tokens. A desktop client must not embed an App private key.
GitHub App installation tokens are not implemented in this desktop build;
Projects v2 requires the token to have Projects read permission.

## AI providers

Open **Settings → Providers**. API keys use the same OS keychain boundary.
OpenAI, Anthropic, Gemini, an OpenAI-compatible endpoint and Ollama each have a
separate protocol adapter. The built-in mock provider keeps review and
explanation flows deterministic without external transfer.

External AI is denied by default. Enable it in **Settings → Privacy &
permissions** only after deciding which repository evidence may leave the host.

## Local Git

Open **Local workspace**, paste an absolute Git working-tree path and connect.
Inspection is read-only. Documentation indexing is explicit. Command execution
is disabled by default and requires the local-execution permission. Choose
`host`, `docker` or `podman` under **Settings → Sandbox**. Container mode
requires a configured image and a running runtime; source is mounted read-only
unless file writes are separately allowed.

## Optional webhook receiver

See [AUTOMATIONS.md](AUTOMATIONS.md) for the separate HMAC-verified
`gitpilot-webhook` receiver. It must be deployed behind TLS and never replaces
the desktop app's polling fallback.

## Data locations

- macOS: `~/Library/Application Support/com.gitpilot.GitPilot/`
- data override: `GITPILOT_DATA_DIR`
- database: `gitpilot.sqlite`
- log directory: `logs/`

Use **Settings → Data → Backup now** for a `VACUUM INTO` snapshot. Use the
typed **DELETE** dialog to remove local records. Disconnect credentials
separately in the GitHub settings.

