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

[ "$(uname -s)" = "Darwin" ] || release_die "macOS packaging must run on macOS"
[ "$(uname -m)" = "arm64" ] || release_die "macOS v1 packaging requires an Apple Silicon host"
for command in cargo codesign ditto hdiutil lipo node npm rustup shasum; do
  require_command "$command"
done

reject_hardcoded_apple_ids "$ROOT"
verify_macos_print_contract "$ROOT"
"$ROOT/scripts/fetch-pdfium.sh"
verify_pdfium_receipt "$ROOT"

if [ "$mode" = "signed" ]; then
  verify_clean_release_tree "$ROOT"
  : "${CODESIGN_IDENTITY:?CODESIGN_IDENTITY must name a Developer ID Application identity}"
  printf '%s\n' "$CODESIGN_IDENTITY" | grep -q '^Developer ID Application:' ||
    release_die "CODESIGN_IDENTITY must be a Developer ID Application identity"
  security find-identity -v -p codesigning | grep -Fq "\"$CODESIGN_IDENTITY\"" ||
    release_die "CODESIGN_IDENTITY is unavailable in the active keychains"
else
  printf '%s\n' 'UNSIGNED SMOKE ONLY: no Developer ID signature or notarization is claimed.'
  if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" ]; then
    printf '%s\n' 'UNSIGNED SMOKE: dirty worktree accepted only for local validation; artifact is not publishable.'
  fi
fi

rustup target add aarch64-apple-darwin
rm -rf "$ROOT/dist/macos-arm64"
mkdir -p "$ROOT/dist/macos-arm64"

CI=true npm exec --workspace @mdviewer/desktop tauri -- build \
  --ci \
  --target aarch64-apple-darwin \
  --bundles app \
  --no-sign

built_app="$ROOT/target/aarch64-apple-darwin/release/bundle/macos/MDViewer.app"
app="$ROOT/dist/macos-arm64/MDViewer.app"
test -d "$built_app" || release_die "Tauri did not produce the expected application bundle"
ditto "$built_app" "$app"

executable="$app/Contents/MacOS/mdviewer-desktop"
pdfium="$app/Contents/Resources/lib/libpdfium.dylib"
require_arm64_file "$executable"
require_arm64_file "$pdfium"

version="$(release_version "$ROOT")"
dmg="$ROOT/dist/macos-arm64/MDViewer-$version-arm64.dmg"

if [ "$mode" = "signed" ]; then
  codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$pdfium"
  codesign --verify --strict --verbose=2 "$pdfium"
  codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$app"
  verify_developer_id_app "$app"
fi

create_dmg "$app" "$dmg" "MDViewer $version"
if [ "$mode" = "signed" ]; then
  codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$dmg"
  codesign --verify --strict --verbose=2 "$dmg"
fi

commit="$(git -C "$ROOT" rev-parse HEAD)"
node - "$mode" "$commit" "$executable" "$pdfium" "$dmg" "$ROOT/dist/macos-arm64/package-receipt.json" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const [mode, commit, executable, pdfium, dmg, output] = process.argv.slice(2);
const sha256 = (path) => crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex');
fs.writeFileSync(output, `${JSON.stringify({
  schemaVersion: 1,
  mode,
  publishable: false,
  signed: mode === 'signed',
  notarized: false,
  target: 'aarch64-apple-darwin',
  commit,
  artifacts: {
    executableSha256: sha256(executable),
    pdfiumSha256: sha256(pdfium),
    dmgSha256: sha256(dmg),
  },
}, null, 2)}\n`);
NODE

if [ "$mode" = "unsigned-smoke" ]; then
  printf 'UNSIGNED SMOKE COMPLETE: arm64 app contents and DMG container verified; signature/notary gates intentionally not run.\n'
else
  printf 'SIGNED PACKAGE COMPLETE: notarization is still required before publication.\n'
fi
printf 'app: %s\ndmg: %s\n' "$app" "$dmg"
