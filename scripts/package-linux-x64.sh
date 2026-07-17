#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=release-common.sh
. "$ROOT/scripts/release-common.sh"

test "$(uname -s)" = Linux || release_die "Linux packaging requires a Linux host"
test "$(uname -m)" = x86_64 || release_die "Linux packaging requires x86_64"
verify_clean_release_tree "$ROOT"

version="$(node -p "require('$ROOT/apps/desktop/package.json').version")"
test "$version" = "1.2.1" || release_die "Linux release version must be 1.2.1"
tesseract_major="$(tesseract --version 2>&1 | sed -n '1s/^tesseract \([0-9]*\).*/\1/p')"
test "$tesseract_major" = 5 || release_die "Linux packaging requires Tesseract major version 5"

"$ROOT/scripts/fetch-tessdata.sh"
export APPIMAGE_EXTRACT_AND_RUN=1
export CARGO_TARGET_DIR="$ROOT/.cache/target-linux-x64"
npm exec --workspace @mdviewer/desktop tauri -- build --ci \
  --bundles appimage,deb \
  --config src-tauri/tauri.linux.conf.json

mapfile -t appimages < <(find "$CARGO_TARGET_DIR/release/bundle/appimage" -maxdepth 1 -type f -name '*.AppImage' -print)
mapfile -t debs < <(find "$CARGO_TARGET_DIR/release/bundle/deb" -maxdepth 1 -type f -name '*.deb' -print)
test "${#appimages[@]}" -eq 1 || release_die "expected exactly one AppImage"
test "${#debs[@]}" -eq 1 || release_die "expected exactly one Debian package"

dist="$ROOT/dist/linux-x64"
mkdir -p "$dist"
appimage="$dist/MDViewer-$version-x86_64.AppImage"
deb="$dist/MDViewer-$version-amd64.deb"
install -m 0755 "${appimages[0]}" "$appimage"
install -m 0644 "${debs[0]}" "$deb"
commit="$(git -C "$ROOT" rev-parse HEAD)"

node - "$version" "$commit" "$appimage" "$deb" "$dist/package-receipt-linux-x64.json" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const [version, commit, appimage, deb, receipt] = process.argv.slice(2);
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
fs.writeFileSync(receipt, JSON.stringify({
  schemaVersion: 1,
  platform: 'linux',
  version,
  target: 'x86_64-unknown-linux-gnu',
  publishable: false,
  signed: false,
  provenance: 'pending_github_attestation',
  commit,
  artifacts: {
    appimage: { name: path.basename(appimage), sha256: sha256(appimage) },
    deb: { name: path.basename(deb), sha256: sha256(deb) },
  },
}, null, 2) + '\n');
NODE

printf 'LINUX PACKAGE COMPLETE: production verification is still required.\n'
