#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_PATH="$ROOT_DIR/MDViewer.xcodeproj"
ARCHIVE_PATH="$ROOT_DIR/.build/MDViewer-AppStore.xcarchive"
EXPORT_PATH="$ROOT_DIR/.build/MDViewer-AppStoreExport"
AUTH_KEY_PATH="${AUTH_KEY_PATH:-}"
AUTH_KEY_ID="${AUTH_KEY_ID:-}"
AUTH_ISSUER_ID="${AUTH_ISSUER_ID:-}"

: "${AUTH_KEY_PATH:?Set AUTH_KEY_PATH to the App Store Connect private key path}"
: "${AUTH_KEY_ID:?Set AUTH_KEY_ID to the App Store Connect key ID}"
: "${AUTH_ISSUER_ID:?Set AUTH_ISSUER_ID to the App Store Connect issuer ID}"

xcodegen generate

rm -rf "$ARCHIVE_PATH" "$EXPORT_PATH"

xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme MDViewer \
  -configuration Release \
  -destination "generic/platform=macOS" \
  -archivePath "$ARCHIVE_PATH" \
  -derivedDataPath "$ROOT_DIR/.build/XcodeArchiveData" \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$AUTH_KEY_PATH" \
  -authenticationKeyID "$AUTH_KEY_ID" \
  -authenticationKeyIssuerID "$AUTH_ISSUER_ID" \
  archive

xcodebuild \
  -exportArchive \
  -archivePath "$ARCHIVE_PATH" \
  -exportPath "$EXPORT_PATH" \
  -exportOptionsPlist "$ROOT_DIR/macos/ExportOptions-AppStore.plist" \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$AUTH_KEY_PATH" \
  -authenticationKeyID "$AUTH_KEY_ID" \
  -authenticationKeyIssuerID "$AUTH_ISSUER_ID"

printf "Archive: %s\n" "$ARCHIVE_PATH"
printf "Export: %s\n" "$EXPORT_PATH"
