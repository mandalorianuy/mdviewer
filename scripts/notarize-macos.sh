#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=release-common.sh
. "$ROOT/scripts/release-common.sh"

[ "$#" -eq 0 ] || { echo "usage: $0" >&2; exit 2; }
[ "$(uname -s)" = "Darwin" ] || release_die "notarization must run on macOS"
[ "$(uname -m)" = "arm64" ] || release_die "macOS v1 notarization requires Apple Silicon"
verify_clean_release_tree "$ROOT"
reject_hardcoded_apple_ids "$ROOT"

version="$(release_version "$ROOT")"
app="$ROOT/dist/macos-arm64/MDViewer.app"
dmg="$ROOT/dist/macos-arm64/MDViewer-$version-arm64.dmg"
receipt="$ROOT/dist/macos-arm64/package-receipt.json"
verify_developer_id_app "$app"
test -f "$dmg" || release_die "signed DMG is missing"
codesign --verify --strict --verbose=2 "$app/Contents/Resources/lib/libpdfium.dylib"
codesign --verify --strict --verbose=2 "$dmg"

receipt_state="$(node -e 'const r=require(process.argv[1]); process.stdout.write(String(r.notarized) + ":" + String(r.publishable))' "$receipt")" ||
  release_die "package receipt is invalid"
case "$receipt_state" in
  false:false)
    verify_package_receipt "$ROOT" "$receipt" signed "$app" "$dmg" false false
    ;;
  true:false)
    verify_package_receipt "$ROOT" "$receipt" signed "$app" "$dmg" true false
    xcrun stapler validate "$app"
    xcrun stapler validate "$dmg"
    printf 'NOTARIZATION ALREADY COMPLETE: receipt remains pending production verification.\n'
    exit 0
    ;;
  *)
    release_die "package receipt is not a pending signed or notarized release"
    ;;
esac

: "${APPLE_API_KEY_PATH:?APPLE_API_KEY_PATH is required}"
: "${APPLE_API_KEY:?APPLE_API_KEY is required}"
: "${APPLE_API_ISSUER:?APPLE_API_ISSUER is required}"
: "${CODESIGN_IDENTITY:?CODESIGN_IDENTITY is required to recreate the DMG}"
test -f "$APPLE_API_KEY_PATH" || release_die "APPLE_API_KEY_PATH does not point to a file"

zip="$ROOT/dist/macos-arm64/MDViewer-$version-notarization.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"

submit() {
  artifact="$1"
  output="$2"
  xcrun notarytool submit "$artifact" \
    --key "$APPLE_API_KEY_PATH" \
    --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" \
    --wait \
    --output-format json >"$output"
  node -e 'const r=require(process.argv[1]); if (r.status !== "Accepted") process.exit(1)' "$output"
}

submit "$zip" "$ROOT/dist/macos-arm64/notary-app.json"
xcrun stapler staple "$app"
xcrun stapler validate "$app"

# Recreate the disk image so it contains the stapled application, then sign and notarize it.
create_dmg "$app" "$dmg" "MDViewer $version"
codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
submit "$dmg" "$ROOT/dist/macos-arm64/notary-dmg.json"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"

mark_package_receipt_notarized "$receipt" "$dmg" "$ROOT" "$app"
verify_package_receipt "$ROOT" "$receipt" signed "$app" "$dmg" true false

printf 'NOTARIZATION COMPLETE: app and DMG tickets are stapled; receipt is pending production verification.\n'
