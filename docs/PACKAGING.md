# Packaging and distribution

## macOS

Build an unsigned development app bundle:

```bash
cargo build --release -p gitpilot-app
./scripts/package-macos.sh
```

The output is `dist/GitPilot.app`. It is **not** a trust-marked release target.
Signing and notarization require an Apple Developer identity:

```bash
codesign --deep --force --options runtime \
  --sign "Developer ID Application: YOUR NAME (TEAMID)" dist/GitPilot.app
xcrun notarytool submit dist/GitPilot.zip \
  --apple-id "$APPLE_ID" --team-id "$TEAM_ID" --password "$APP_PASSWORD" --wait
xcrun stapler staple dist/GitPilot.app
```

Store notarization credentials in the keychain, not in this repository.

## Windows and Linux

The UI and core are cross-platform Rust. Remaining platform work:

- Windows builds use the Windows Credential Manager adapter (compile-gated; not
  runtime-tested on this macOS host). Linux builds refuse to persist a
  credential until a reviewed Secret Service backend is added rather than
  falling back to plaintext.
- Validate filesystem path handling and packaging on each target.
- Add native application manifests and code signing.
- Validate process/container isolation behavior, which is host-specific.

Recommended Windows packaging: `cargo build --release`, then a WiX or MSIX
installer. Recommended Linux packaging: a `.deb`/AppImage with an explicit
credential-store dependency. Neither is produced by this source tree yet.

The optional webhook receiver is a second binary:

```bash
cargo build --release -p gitpilot-app --bin gitpilot-webhook
```

Deploy it separately behind TLS and a network policy. It stores verified events
in the configured GitPilot data directory and does not open a public listener
from the desktop binary.

## Upgrade and migration behavior

The SQLite migration is idempotent and versioned in `schema_migrations`.
Back up the database before a release upgrade. Do not ship a binary with a
different embedded schema migration unless it has been tested against a copy of
an existing data directory.

## Automatic updates

Not implemented. A secure updater needs signed manifests, rollback and TUF-like
metadata; the application does not silently download or execute updates.

