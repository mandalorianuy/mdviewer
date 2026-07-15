#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_BUNDLE="$ROOT_DIR/dist/MDViewer.app"
INSTALL_DIR="${1:-/Applications}"
DEST_APP="$INSTALL_DIR/MDViewer.app"
BUNDLE_ID="com.facundo.mdviewer"

if [[ ! -d "$APP_BUNDLE" ]]; then
  echo "No encontré $APP_BUNDLE. Ejecutá primero: ./scripts/package-app.sh"
  exit 1
fi

mkdir -p "$INSTALL_DIR"
rm -rf "$DEST_APP"
cp -R "$APP_BUNDLE" "$DEST_APP"

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [[ -x "$LSREGISTER" ]]; then
  "$LSREGISTER" -f "$DEST_APP" >/dev/null 2>&1 || true
fi

if command -v duti >/dev/null 2>&1; then
  duti -s "$BUNDLE_ID" .md all || true
  duti -s "$BUNDLE_ID" .markdown all || true
fi

echo "Instalada en: $DEST_APP"
if ! command -v duti >/dev/null 2>&1; then
  echo "Tip: instalá duti para setear asociación por defecto automática (.md)."
fi
