#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_PATH="$ROOT_DIR/MDViewer.xcodeproj"
ARCHIVE_PATH="$ROOT_DIR/dist/MDViewer.xcarchive"
APP_PATH="$ARCHIVE_PATH/Products/Applications/MDViewer.app"
DEVELOPMENT_TEAM_ID="${DEVELOPMENT_TEAM_ID:-}"
SIGNING_IDENTITY="${SIGNING_IDENTITY:-}"

if [[ -z "$DEVELOPMENT_TEAM_ID" ]]; then
  echo "Set DEVELOPMENT_TEAM_ID to your Apple Developer Team ID." >&2
  exit 2
fi

if [[ -z "$SIGNING_IDENTITY" ]]; then
  echo "Set SIGNING_IDENTITY to a Developer ID Application identity (e.g. 'Developer ID Application: Your Name (TEAM_ID)')." >&2
  exit 2
fi

if [[ ! -d "$PROJECT_PATH" ]]; then
  echo "Generating Xcode project..."
  xcodegen generate
fi

mkdir -p "$ROOT_DIR/dist"
rm -rf "$ARCHIVE_PATH"

echo "==> Archiving signed MDViewer app"
xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme MDViewer \
  -configuration Release \
  -destination "platform=macOS" \
  -archivePath "$ARCHIVE_PATH" \
  DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM_ID" \
  CODE_SIGN_STYLE=Manual \
  CODE_SIGN_IDENTITY="$SIGNING_IDENTITY" \
  archive

echo "==> Archived app: $APP_PATH"
