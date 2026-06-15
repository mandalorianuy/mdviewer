#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="${APP_PATH:-$ROOT_DIR/dist/MDViewer.xcarchive/Products/Applications/MDViewer.app}"

if [[ ! -d "$APP_PATH" ]]; then
  echo "App not found at $APP_PATH" >&2
  exit 2
fi

echo "==> Gatekeeper assessment"
spctl --assess --type execute -vv "$APP_PATH"

echo
echo "==> Code signature verification"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

echo
echo "==> Notarization stapled ticket"
stapler validate "$APP_PATH" || true
