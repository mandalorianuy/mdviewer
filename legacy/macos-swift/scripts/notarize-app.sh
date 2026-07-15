#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCHIVE_PATH="${ARCHIVE_PATH:-$ROOT_DIR/dist/MDViewer.xcarchive}"
APP_PATH="${APP_PATH:-$ARCHIVE_PATH/Products/Applications/MDViewer.app}"
ZIP_PATH="${ZIP_PATH:-$ROOT_DIR/dist/MDViewer-notarization.zip}"
NOTARY_PROFILE="${NOTARY_PROFILE:-}"

if [[ -z "$NOTARY_PROFILE" ]]; then
  echo "Set NOTARY_PROFILE to a notarytool keychain profile name." >&2
  exit 2
fi

if [[ ! -d "$APP_PATH" ]]; then
  echo "App not found at $APP_PATH; archiving first..."
  "$ROOT_DIR/scripts/archive-signed-app.sh" >/dev/null
fi

echo "==> Zipping app for notarization"
rm -f "$ZIP_PATH"
/usr/bin/ditto -c -k --keepParent "$APP_PATH" "$ZIP_PATH"

SUBMISSION_JSON="$ROOT_DIR/dist/notary_submission.json"

echo "==> Submitting to Apple notary service"
xcrun notarytool submit "$ZIP_PATH" \
  --keychain-profile "$NOTARY_PROFILE" \
  --wait \
  --output-format json > "$SUBMISSION_JSON"

echo "==> Stapling notarization ticket to app"
xcrun stapler staple "$APP_PATH"

echo "==> Notarization complete"
echo "  app: $APP_PATH"
echo "  zip: $ZIP_PATH"
echo "  submission: $SUBMISSION_JSON"
