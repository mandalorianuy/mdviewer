#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_BUNDLE="$DIST_DIR/MDViewer.app"
STAGING_DIR="$ROOT_DIR/.build/dmg-staging"
VOLUME_NAME="MDViewer"

printf "==> Packaging app bundle\n"
"$ROOT_DIR/scripts/package-app.sh"

VERSION="$(
  /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP_BUNDLE/Contents/Info.plist"
)"
DMG_PATH="$DIST_DIR/MDViewer-${VERSION}.dmg"

printf "==> Preparing DMG staging directory\n"
rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR"

cp -R "$APP_BUNDLE" "$STAGING_DIR/MDViewer.app"
ln -s /Applications "$STAGING_DIR/Applications"

printf "==> Creating DMG\n"
rm -f "$DMG_PATH"
hdiutil create \
  -volname "$VOLUME_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

printf "Done: %s\n" "$DMG_PATH"
