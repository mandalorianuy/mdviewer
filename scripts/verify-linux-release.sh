#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=release-common.sh
. "$ROOT/scripts/release-common.sh"

test "$(uname -s)" = Linux || release_die "Linux verification requires a Linux host"
test "$(uname -m)" = x86_64 || release_die "Linux verification requires x86_64"
verify_clean_release_tree "$ROOT"
export APPIMAGE_EXTRACT_AND_RUN=1

dist="$ROOT/dist/linux-x64"
receipt="$dist/package-receipt.json"
test -f "$receipt" || release_die "Linux package receipt is missing"

readarray -t receipt_values < <(node - "$ROOT" "$receipt" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const [root, receiptPath] = process.argv.slice(2);
const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
const fail = (message) => { console.error(message); process.exit(1); };
if (receipt.schemaVersion !== 1 || receipt.platform !== 'linux') fail('invalid Linux receipt');
if (receipt.version !== '1.2.1' || receipt.target !== 'x86_64-unknown-linux-gnu') fail('unexpected Linux release identity');
if (receipt.publishable || receipt.signed || receipt.provenance !== 'pending_github_attestation') fail('invalid pre-verification state');
if (receipt.commit !== execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()) fail('stale Linux receipt');
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
for (const key of ['appimage', 'deb']) {
  const artifact = receipt.artifacts?.[key];
  const file = path.join(path.dirname(receiptPath), artifact?.name ?? '');
  if (!artifact || !fs.statSync(file).isFile() || sha256(file) !== artifact.sha256) fail('invalid ' + key + ' artifact');
  console.log(file);
}
NODE
)
appimage="${receipt_values[0]}"
deb="${receipt_values[1]}"

temporary="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-linux-verify.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT
(
  cd "$temporary"
  "$appimage" --appimage-extract >/dev/null
)
appdir="$temporary/squashfs-root"
for language in eng spa; do
  packaged="$appdir/usr/share/mdviewer/tessdata/$language.traineddata"
  expected="$(awk -v file="$language.traineddata" '$2 == file {print $1}' "$ROOT/.cache/tessdata/4.1.0/SHA256SUMS")"
  test -n "$expected" && test "$(sha256sum "$packaged" | awk '{print $1}')" = "$expected" ||
    release_die "AppImage $language tessdata is missing or changed"
done
find "$appdir" -type f -name 'libtesseract.so*' -print | grep -q libtesseract ||
  release_die "AppImage does not bundle Tesseract"
find "$appdir" -type f -name 'liblept.so*' -print | grep -q liblept ||
  release_die "AppImage does not bundle Leptonica"

dependencies="$(dpkg-deb -f "$deb" Depends)"
for dependency in libtesseract5 liblept5 tesseract-ocr-eng tesseract-ocr-spa; do
  printf '%s\n' "$dependencies" | grep -Eq "(^|, )[[:space:]]*$dependency([[:space:]]|,|$)" ||
    release_die "Debian package is missing dependency: $dependency"
done

cargo test -p mdconvert-ocr --test vision_macos \
  local_backend_recognizes_a_stable_png_and_returns_normalized_bounds

set +e
timeout 8s xvfb-run -a "$appimage" >"$temporary/appimage.log" 2>&1
appimage_status=$?
set -e
test "$appimage_status" -eq 124 || {
  cat "$temporary/appimage.log" >&2
  release_die "AppImage did not remain running under Xvfb"
}

test "$(id -u)" -eq 0 || release_die "Debian install smoke requires an isolated root runner"
dpkg -i "$deb"
package_name="$(dpkg-deb -f "$deb" Package)"
installed_binary="$(dpkg -L "$package_name" | awk '/\/usr\/bin\// {print; exit}')"
test -x "$installed_binary" || release_die "Debian package did not install its executable"
set +e
timeout 8s xvfb-run -a "$installed_binary" >"$temporary/deb.log" 2>&1
deb_status=$?
set -e
test "$deb_status" -eq 124 || {
  cat "$temporary/deb.log" >&2
  release_die "Debian-installed app did not remain running under Xvfb"
}
dpkg -r "$package_name"

node - "$ROOT" "$receipt" "$appimage" "$deb" <<'NODE'
const crypto = require('node:crypto');
const fs = require('node:fs');
const { execFileSync } = require('node:child_process');
const [root, receiptPath, appimage, deb] = process.argv.slice(2);
const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
if (receipt.publishable || receipt.commit !== execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()) process.exit(1);
if (receipt.artifacts.appimage.sha256 !== sha256(appimage) || receipt.artifacts.deb.sha256 !== sha256(deb)) process.exit(1);
receipt.publishable = true;
receipt.provenance = 'github_attestation_required';
const temporary = receiptPath + '.tmp';
fs.writeFileSync(temporary, JSON.stringify(receipt, null, 2) + '\n', { mode: 0o600 });
fs.renameSync(temporary, receiptPath);
NODE

printf 'LINUX RELEASE VERIFIED: AppImage, Debian install, OCR runtime and checksums pass.\n'
