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
if [ "$mode" = "unsigned-smoke" ]; then
  verify_package_receipt "$ROOT" "$receipt" "$mode" "$app" "$dmg" false false
else
  verify_package_receipt "$ROOT" "$receipt" "$mode" "$app" "$dmg" true false
fi

mount="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-verify-dmg.XXXXXX")"
cleanup() {
  hdiutil detach "$mount" -quiet >/dev/null 2>&1 || true
  rmdir "$mount" >/dev/null 2>&1 || true
}
trap cleanup EXIT
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$dmg"
verify_mounted_release_contents "$receipt" "$app" "$mount/MDViewer.app" "$mount/Applications"

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

run_signed_release_gates() {
  local root="$1"
  : "$2"
  local exterior_app="$3"
  local exterior_dmg="$4"
  local mounted_app="$mount/MDViewer.app"
  local alias_home workflow

  verify_clean_release_tree "$root" || return 1
  verify_developer_id_app "$exterior_app" || return 1
  verify_developer_id_app "$mounted_app" || return 1
  codesign --verify --strict --verbose=2 "$exterior_app/Contents/Resources/lib/libpdfium.dylib" || return 1
  codesign --verify --strict --verbose=2 "$mounted_app/Contents/Resources/lib/libpdfium.dylib" || return 1
  codesign --verify --strict --verbose=2 "$exterior_dmg" || return 1
  xcrun stapler validate "$exterior_app" || return 1
  xcrun stapler validate "$mounted_app" || return 1
  xcrun stapler validate "$exterior_dmg" || return 1
  spctl --assess --type execute --verbose=4 "$exterior_app" || return 1
  spctl --assess --type execute --verbose=4 "$mounted_app" || return 1
  spctl --assess --type open --context context:primary-signature --verbose=4 "$exterior_dmg" || return 1

  env -u PDFIUM_DYNAMIC_LIB_PATH \
  MDVIEWER_APPLICATION_BUNDLE="$exterior_app" \
  cargo test -p mdviewer-desktop --test macos_integration \
    signed_application_uses_its_bundled_pdfium_without_environment_configuration \
    -- --ignored --exact || return 1

  alias_home="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-alias-gate.XXXXXX")"
  HOME="$alias_home" \
  MDVIEWER_CONFIRM_REAL_WORKFLOW_INSTALL=yes \
  MDVIEWER_APPLICATION_BUNDLE="$exterior_app" \
  cargo test -p mdviewer-desktop --test macos_integration \
    installs_the_exact_embedded_development_application_alias -- --ignored --exact || {
      rm -rf "$alias_home"
      return 1
    }
  workflow="$alias_home/Library/PDF Services/Guardar como Markdown con MDViewer"
  test "$(file -b "$workflow")" = "MacOS Alias file" || {
    rm -rf "$alias_home"
    release_die "installed PDF Service is not a native macOS alias"
    return 1
  }
  rm -rf "$alias_home"
}

verify_then_publish_release "$ROOT" "$receipt" "$app" "$dmg" run_signed_release_gates

printf 'SIGNED RELEASE VERIFIED: Developer ID, exact native alias lifecycle, notarization, stapling, Gatekeeper and arm64-only contents pass.\n'
