#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/.build/release"
DIST_DIR="$ROOT_DIR/dist"
APP_BUNDLE="$DIST_DIR/MDViewer.app"
ICON_FILE="$ROOT_DIR/macos/AppIcon.icns"
DOCUMENT_ICON_FILE="$ROOT_DIR/macos/MarkdownDocument.icns"

printf "==> Generating app icon\n"
swift "$ROOT_DIR/scripts/generate_icon.swift"

printf "==> Building release binary\n"
swift build -c release

printf "==> Packaging app bundle\n"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$BUILD_DIR/MDViewer" "$APP_BUNDLE/Contents/MacOS/MDViewer"
chmod +x "$APP_BUNDLE/Contents/MacOS/MDViewer"
cp "$ROOT_DIR/macos/Info.plist" "$APP_BUNDLE/Contents/Info.plist"
cp "$ICON_FILE" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
cp "$DOCUMENT_ICON_FILE" "$APP_BUNDLE/Contents/Resources/MarkdownDocument.icns"

printf "==> Applying ad-hoc signature\n"
codesign --force --deep --sign - "$APP_BUNDLE"

printf "Done: %s\n" "$APP_BUNDLE"
