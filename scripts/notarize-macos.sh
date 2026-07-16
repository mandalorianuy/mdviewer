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

: "${APPLE_API_KEY_PATH:?APPLE_API_KEY_PATH is required}"
: "${APPLE_API_KEY:?APPLE_API_KEY is required}"
: "${APPLE_API_ISSUER:?APPLE_API_ISSUER is required}"
: "${CODESIGN_IDENTITY:?CODESIGN_IDENTITY is required to recreate the DMG}"
test -f "$APPLE_API_KEY_PATH" || release_die "APPLE_API_KEY_PATH does not point to a file"

version="$(release_version "$ROOT")"
app="$ROOT/dist/macos-arm64/MDViewer.app"
dmg="$ROOT/dist/macos-arm64/MDViewer-$version-arm64.dmg"
receipt="$ROOT/dist/macos-arm64/package-receipt.json"
verify_developer_id_app "$app"
test -f "$dmg" || release_die "signed DMG is missing"
node -e 'const r=require(process.argv[1]); if (!r.signed || r.mode !== "signed" || r.publishable || r.notarized) process.exit(1)' "$receipt" ||
  release_die "package receipt is not a pending signed release"

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

node - "$receipt" "$dmg" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const [receiptPath, dmgPath] = process.argv.slice(2);
const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
receipt.notarized = true;
receipt.publishable = true;
receipt.artifacts.dmgSha256 = crypto.createHash('sha256').update(fs.readFileSync(dmgPath)).digest('hex');
fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
NODE

printf 'NOTARIZATION COMPLETE: app and DMG tickets are stapled.\n'
