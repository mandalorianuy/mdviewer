#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_PATH="$ROOT_DIR/MDViewer.xcodeproj"
ARCHIVE_PATH="$ROOT_DIR/.build/MDViewer-AppStore.xcarchive"
EXPORT_PATH="$ROOT_DIR/.build/MDViewer-AppStoreExport"
AUTH_KEY_PATH="${AUTH_KEY_PATH:-$HOME/.appstoreconnect/private_keys/AuthKey_J3JJ2WXQ5S.p8}"
AUTH_KEY_ID="${AUTH_KEY_ID:-J3JJ2WXQ5S}"
AUTH_ISSUER_ID="${AUTH_ISSUER_ID:-c9f7eed4-57f2-4c22-8efa-8e2cf829a79e}"

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
