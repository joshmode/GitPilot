#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${ROOT}/dist/GitPilot.app"
BIN="${ROOT}/target/release/gitpilot"

cd "$ROOT"
cargo build --release -p gitpilot-app

rm -rf "$APP"
mkdir -p "${APP}/Contents/MacOS" "${APP}/Contents/Resources"
cp "$BIN" "${APP}/Contents/MacOS/GitPilot"
chmod +x "${APP}/Contents/MacOS/GitPilot"
printf 'APPL????' > "${APP}/Contents/PkgInfo"

cat > "${APP}/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>GitPilot</string>
  <key>CFBundleExecutable</key><string>GitPilot</string>
  <key>CFBundleIdentifier</key><string>com.gitpilot.GitPilot</string>
  <key>CFBundleName</key><string>GitPilot</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

printf 'Built %s\n' "$APP"
printf 'Unsigned development bundle. See docs/PACKAGING.md before distributing.\n'

