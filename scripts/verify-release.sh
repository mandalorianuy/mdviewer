#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=release-common.sh
. "$ROOT/scripts/release-common.sh"

mode="signed"
case "${1:-}" in
  '') ;;
  --unsigned-smoke) mode="unsigned-smoke" ;;
  *) echo "usage: $0 [--unsigned-smoke]" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "usage: $0 [--unsigned-smoke]" >&2; exit 2; }

[ "$(uname -s)" = "Darwin" ] || release_die "macOS release verification must run on macOS"
[ "$(uname -m)" = "arm64" ] || release_die "macOS v1 verification requires Apple Silicon"
reject_hardcoded_apple_ids "$ROOT"
verify_macos_print_contract "$ROOT"

version="$(release_version "$ROOT")"
app="$ROOT/dist/macos-arm64/MDViewer.app"
dmg="$ROOT/dist/macos-arm64/MDViewer-$version-arm64.dmg"
receipt="$ROOT/dist/macos-arm64/package-receipt.json"
test -f "$receipt" || release_die "package receipt is missing"
test -f "$dmg" || release_die "DMG is missing"

verify_unsigned_app "$app"
require_arm64_file "$app/Contents/MacOS/mdviewer-desktop"
require_arm64_file "$app/Contents/Resources/lib/libpdfium.dylib"

node - "$receipt" "$mode" "$app/Contents/MacOS/mdviewer-desktop" "$app/Contents/Resources/lib/libpdfium.dylib" "$dmg" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const [receiptPath, expectedMode, executable, pdfium, dmg] = process.argv.slice(2);
const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
const sha256 = (path) => crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex');
if (receipt.mode !== expectedMode || receipt.target !== 'aarch64-apple-darwin') process.exit(1);
if (receipt.artifacts?.executableSha256 !== sha256(executable)) process.exit(1);
if (receipt.artifacts?.pdfiumSha256 !== sha256(pdfium)) process.exit(1);
if (receipt.artifacts?.dmgSha256 !== sha256(dmg)) process.exit(1);
NODE

mount="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-verify-dmg.XXXXXX")"
cleanup() {
  hdiutil detach "$mount" -quiet >/dev/null 2>&1 || true
  rmdir "$mount" >/dev/null 2>&1 || true
}
trap cleanup EXIT
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$dmg"
test -d "$mount/MDViewer.app" || release_die "DMG does not contain MDViewer.app"
test -L "$mount/Applications" || release_die "DMG does not contain the Applications link"
require_arm64_file "$mount/MDViewer.app/Contents/MacOS/mdviewer-desktop"
require_arm64_file "$mount/MDViewer.app/Contents/Resources/lib/libpdfium.dylib"

if [ "$mode" = "unsigned-smoke" ]; then
  node -e 'const r=require(process.argv[1]); if (r.publishable || r.signed || r.notarized) process.exit(1)' "$receipt" ||
    release_die "unsigned smoke receipt makes a signing or notarization claim"
  if codesign --verify --strict "$app" >/dev/null 2>&1; then
    release_die "unsigned smoke unexpectedly contains a signed outer application"
  fi
  printf 'UNSIGNED SMOKE VERIFIED: arm64-only app, bundled PDFium, PDF open-event metadata and DMG contents pass.\n'
  printf 'NOT VERIFIED IN THIS MODE: Developer ID signature, native alias ownership, notarization, stapling and Gatekeeper.\n'
  exit 0
fi

verify_clean_release_tree "$ROOT"
verify_developer_id_app "$app"
codesign --verify --strict --verbose=2 "$app/Contents/Resources/lib/libpdfium.dylib"
codesign --verify --strict --verbose=2 "$dmg"
xcrun stapler validate "$app"
xcrun stapler validate "$dmg"
spctl --assess --type execute --verbose=4 "$app"
spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg"
node -e 'const r=require(process.argv[1]); if (!r.publishable || !r.signed || !r.notarized) process.exit(1)' "$receipt" ||
  release_die "release receipt does not attest signed and notarized production mode"

env -u PDFIUM_DYNAMIC_LIB_PATH \
MDVIEWER_APPLICATION_BUNDLE="$app" \
cargo test -p mdviewer-desktop --test macos_integration \
  signed_application_uses_its_bundled_pdfium_without_environment_configuration \
  -- --ignored --exact

alias_home="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-alias-gate.XXXXXX")"
HOME="$alias_home" \
MDVIEWER_CONFIRM_REAL_WORKFLOW_INSTALL=yes \
MDVIEWER_APPLICATION_BUNDLE="$app" \
cargo test -p mdviewer-desktop --test macos_integration \
  installs_the_exact_embedded_development_application_alias -- --ignored --exact
workflow="$alias_home/Library/PDF Services/Guardar como Markdown con MDViewer"
test "$(file -b "$workflow")" = "MacOS Alias file" ||
  release_die "installed PDF Service is not a native macOS alias"
rm -rf "$alias_home"

printf 'SIGNED RELEASE VERIFIED: Developer ID, exact native alias lifecycle, notarization, stapling, Gatekeeper and arm64-only contents pass.\n'
