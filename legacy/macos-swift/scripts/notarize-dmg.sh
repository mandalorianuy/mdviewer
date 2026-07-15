#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_BUNDLE="$DIST_DIR/MDViewer.app"
AUTH_KEY_PATH="${AUTH_KEY_PATH:-$HOME/.appstoreconnect/private_keys/AuthKey_J3JJ2WXQ5S.p8}"
AUTH_KEY_ID="${AUTH_KEY_ID:-J3JJ2WXQ5S}"
AUTH_ISSUER_ID="${AUTH_ISSUER_ID:-c9f7eed4-57f2-4c22-8efa-8e2cf829a79e}"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-}"

if [[ -z "$CODESIGN_IDENTITY" ]]; then
  echo "Seteá CODESIGN_IDENTITY con tu certificado Developer ID Application."
  echo "Ejemplo: CODESIGN_IDENTITY=\"Developer ID Application: Tu Nombre (TEAMID)\" $0"
  exit 1
fi

if ! security find-identity -v -p codesigning | grep -Fq "$CODESIGN_IDENTITY"; then
  echo "No encontré la identidad de firma requerida en el keychain:"
  echo "  $CODESIGN_IDENTITY"
  exit 1
fi

if [[ ! -f "$AUTH_KEY_PATH" ]]; then
  echo "No encontré la key de App Store Connect en $AUTH_KEY_PATH"
  exit 1
fi

printf "==> Building signed app bundle for Developer ID\n"
ENABLE_HARDENED_RUNTIME=1 \
CODESIGN_IDENTITY="$CODESIGN_IDENTITY" \
"$ROOT_DIR/scripts/create-dmg.sh"

VERSION="$(
  /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP_BUNDLE/Contents/Info.plist"
)"
DMG_PATH="$DIST_DIR/MDViewer-${VERSION}.dmg"

printf "==> Signing DMG\n"
codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$DMG_PATH"

printf "==> Submitting DMG for notarization\n"
xcrun notarytool submit "$DMG_PATH" \
  --key "$AUTH_KEY_PATH" \
  --key-id "$AUTH_KEY_ID" \
  --issuer "$AUTH_ISSUER_ID" \
  --wait

printf "==> Stapling notarization ticket\n"
xcrun stapler staple "$APP_BUNDLE"
xcrun stapler staple "$DMG_PATH"

printf "Done: %s\n" "$DMG_PATH"
