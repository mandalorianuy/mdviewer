#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_BUNDLE="$DIST_DIR/MDViewer.app"
PROJECT_PATH="$ROOT_DIR/MDViewer.xcodeproj"
DERIVED_DATA_PATH="$ROOT_DIR/.build/XcodePackageData"
BUILT_APP_BUNDLE="$DERIVED_DATA_PATH/Build/Products/Release/MDViewer.app"
ICON_FILE="$ROOT_DIR/macos/AppIcon.icns"
DOCUMENT_ICON_FILE="$ROOT_DIR/macos/MarkdownDocument.icns"
MODULE_CACHE_DIR="$ROOT_DIR/.build/module-cache"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:--}"
ENABLE_HARDENED_RUNTIME="${ENABLE_HARDENED_RUNTIME:-0}"

mkdir -p "$MODULE_CACHE_DIR"

printf "==> Generating app icon\n"
if [[ "${FORCE_REGENERATE_ICONS:-0}" == "1" || ! -f "$ICON_FILE" || ! -f "$DOCUMENT_ICON_FILE" ]]; then
  swift -module-cache-path "$MODULE_CACHE_DIR" "$ROOT_DIR/scripts/generate_icon.swift"
else
  printf "    using existing icon assets\n"
fi

printf "==> Generating Xcode project\n"
xcodegen generate

printf "==> Building release app bundle\n"
rm -rf "$DERIVED_DATA_PATH"
xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme MDViewer \
  -configuration Release \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  CODE_SIGNING_ALLOWED=NO \
  build

printf "==> Packaging app bundle\n"
rm -rf "$APP_BUNDLE"
cp -R "$BUILT_APP_BUNDLE" "$APP_BUNDLE"

printf "==> Applying code signature\n"
if [[ "$CODESIGN_IDENTITY" == "-" ]]; then
  codesign --force --deep --sign - "$APP_BUNDLE"
else
  SIGN_ARGS=(--force --deep --timestamp --sign "$CODESIGN_IDENTITY")
  if [[ "$ENABLE_HARDENED_RUNTIME" == "1" ]]; then
    SIGN_ARGS+=(--options runtime --entitlements "$ROOT_DIR/macos/MDViewer.entitlements")
  fi
  codesign "${SIGN_ARGS[@]}" "$APP_BUNDLE"
fi

printf "Done: %s\n" "$APP_BUNDLE"
