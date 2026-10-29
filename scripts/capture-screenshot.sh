#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-${ROOT}/docs/screenshot-overview.png}"

mkdir -p "$(dirname "$OUT")"
screencapture -x "$OUT"
printf 'Captured %s\n' "$OUT"

