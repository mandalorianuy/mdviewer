#!/usr/bin/env bash

PDFIUM_RELEASE="chromium-7947"
PDFIUM_ASSET="pdfium-mac-arm64.tgz"
PDFIUM_ARCHIVE_SHA256="aa9739354fc7bc8f200f3f3c9532bd5233298203051e094820272ccd9c997a77"

release_die() {
  printf 'release preflight failed: %s\n' "$*" >&2
  return 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || release_die "required command is unavailable: $1"
}

sha256_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}

require_arm64_file() {
  artifact="$1"
  test -f "$artifact" || release_die "artifact is missing: $artifact" || return 1
  require_command lipo || return 1
  arches="$(lipo -archs "$artifact" 2>/dev/null)" || {
    release_die "artifact is not a Mach-O file: $artifact"
    return 1
  }
  if [ "$arches" != "arm64" ]; then
    release_die "artifact must contain arm64 only, found: $arches"
    return 1
  fi
}

receipt_value() {
  receipt="$1"
  key="$2"
  value="$(sed -n "s/^${key}=//p" "$receipt")"
  if [ -z "$value" ] || [ "$(printf '%s\n' "$value" | wc -l | tr -d ' ')" -ne 1 ]; then
    return 1
  fi
  printf '%s\n' "$value"
}

verify_pdfium_receipt() {
  root="$1"
  cache="$root/.cache/pdfium"
  archive="$cache/$PDFIUM_RELEASE-$PDFIUM_ASSET"
  library="$cache/$PDFIUM_RELEASE/lib/libpdfium.dylib"
  receipt="$cache/$PDFIUM_RELEASE.receipt"

  test -f "$archive" || release_die "pinned PDFium archive is missing" || return 1
  test -f "$library" || release_die "pinned PDFium library is missing" || return 1
  test -f "$receipt" || release_die "PDFium checksum receipt is missing" || return 1
  test ! -L "$archive" && test ! -L "$library" && test ! -L "$receipt" ||
    release_die "PDFium artifacts and receipt must not be symlinks" || return 1

  release="$(receipt_value "$receipt" release)" ||
    { release_die "PDFium receipt has no unique release"; return 1; }
  asset="$(receipt_value "$receipt" asset)" ||
    { release_die "PDFium receipt has no unique asset"; return 1; }
  expected_archive="$(receipt_value "$receipt" archive_sha256)" ||
    { release_die "PDFium receipt has no unique archive checksum"; return 1; }
  expected_library="$(receipt_value "$receipt" library_sha256)" ||
    { release_die "PDFium receipt has no unique library checksum"; return 1; }

  [ "$release" = "$PDFIUM_RELEASE" ] || release_die "unexpected PDFium release in receipt" || return 1
  [ "$asset" = "$PDFIUM_ASSET" ] || release_die "unexpected PDFium asset in receipt" || return 1
  [ "$expected_archive" = "$PDFIUM_ARCHIVE_SHA256" ] ||
    release_die "PDFium archive receipt does not match the pinned checksum" || return 1
  [ "$(sha256_file "$archive")" = "$expected_archive" ] ||
    release_die "PDFium archive checksum does not match its receipt" || return 1
  [ "$(sha256_file "$library")" = "$expected_library" ] ||
    release_die "PDFium library checksum does not match its receipt" || return 1
}

reject_hardcoded_apple_ids() {
  root="$1"
  paths=()
  for candidate in \
    "$root/.github/workflows" \
    "$root/scripts" \
    "$root/legacy/macos-swift/scripts"; do
    if [ -d "$candidate" ]; then
      paths+=("$candidate")
    fi
  done
  [ "${#paths[@]}" -gt 0 ] || return 0

  if ! node - "${paths[@]}" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const patterns = [
  ['key ID assignment', /(?:AUTH_KEY_ID|ASC_KEY_ID|APPLE_API_KEY)\s*=\s*["'][A-Z0-9]{10}["']/],
  ['issuer ID assignment', /(?:AUTH_ISSUER_ID|ASC_ISSUER_ID|APPLE_API_ISSUER)\s*=\s*["'][0-9a-fA-F-]{36}["']/],
  ['key ID default', /(?:AUTH_KEY_ID|ASC_KEY_ID|APPLE_API_KEY)[^\n]{0,120}:-[A-Z0-9]{10}/],
  ['issuer ID default', /(?:AUTH_ISSUER_ID|ASC_ISSUER_ID|APPLE_API_ISSUER)[^\n]{0,160}:-[0-9a-fA-F-]{36}/],
  ['machine-specific private key path', /AuthKey_[A-Z0-9]{10}\.p8/],
];
const excluded = new Set(['test-release-scripts.sh']);
let found = false;
const visit = (candidate) => {
  const stat = fs.lstatSync(candidate);
  if (stat.isSymbolicLink()) return;
  if (stat.isDirectory()) {
    for (const entry of fs.readdirSync(candidate)) visit(path.join(candidate, entry));
    return;
  }
  if (!stat.isFile() || excluded.has(path.basename(candidate))) return;
  const text = fs.readFileSync(candidate, 'utf8');
  for (const [label, pattern] of patterns) {
    if (pattern.test(text)) {
      console.error(`${candidate}: ${label}`);
      found = true;
    }
  }
};
for (const root of process.argv.slice(2)) visit(root);
if (found) process.exit(1);
NODE
  then
    release_die "hardcoded App Store Connect identifier or key path detected"
    return 1
  fi
}

verify_macos_print_contract() {
  root="$1"
  config="$root/apps/desktop/src-tauri/tauri.macos.conf.json"
  test -f "$config" || release_die "macOS Tauri configuration is missing" || return 1
  node - "$config" <<'NODE' || {
const fs = require('node:fs');
const config = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const bundle = config.bundle ?? {};
const association = (bundle.fileAssociations ?? []).find((item) =>
  item.ext?.includes('pdf') &&
  item.contentTypes?.includes('com.adobe.pdf') &&
  item.role === 'Viewer' &&
  item.rank === 'None'
);
const resource = bundle.resources?.['../../../.cache/pdfium/chromium-7947/lib/libpdfium.dylib'];
if (!association || resource !== 'lib/libpdfium.dylib') process.exit(1);
NODE
    release_die "macOS PDF open-event alias or bundled PDFium configuration is incorrect"
    return 1
  }
}

verify_clean_release_tree() {
  root="$1"
  git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
    release_die "release root is not a Git worktree" || return 1

  if ! git -C "$root" diff --quiet -- Cargo.lock package-lock.json ||
     ! git -C "$root" diff --cached --quiet -- Cargo.lock package-lock.json; then
    release_die "Cargo.lock or package-lock.json is modified"
    return 1
  fi
  dirty="$(git -C "$root" status --porcelain --untracked-files=all)"
  if [ -n "$dirty" ]; then
    printf '%s\n' "$dirty" >&2
    release_die "Git worktree is dirty"
    return 1
  fi
}

verify_developer_id_app() {
  app="$1"
  test -d "$app" || release_die "application bundle is missing: $app" || return 1
  require_command codesign || return 1
  codesign --verify --deep --strict --verbose=2 "$app" >/dev/null 2>&1 ||
    release_die "application signature verification failed" || return 1
  details="$(codesign -dv --verbose=4 "$app" 2>&1)" ||
    release_die "application signature metadata is unavailable" || return 1
  printf '%s\n' "$details" | grep -q '^Authority=Developer ID Application:' ||
    release_die "application is not signed with Developer ID Application" || return 1
  printf '%s\n' "$details" | grep -Eq '^TeamIdentifier=[A-Z0-9]{10}$' ||
    release_die "application signature has no valid TeamIdentifier" || return 1
  printf '%s\n' "$details" | grep -Eq '^flags=.*runtime' ||
    release_die "application signature does not enable hardened runtime" || return 1
}

verify_unsigned_app() {
  app="$1"
  test -d "$app" || release_die "application bundle is missing: $app" || return 1
  executable="$app/Contents/MacOS/mdviewer-desktop"
  pdfium="$app/Contents/Resources/lib/libpdfium.dylib"
  require_arm64_file "$executable"
  require_arm64_file "$pdfium"
  test -f "$app/Contents/Info.plist" || release_die "application Info.plist is missing" || return 1
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" |
    grep -qx 'com.mdviewer.desktop' || release_die "unexpected application bundle identifier" || return 1
  /usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes:0:LSHandlerRank' "$app/Contents/Info.plist" |
    grep -qx 'None' || release_die "PDF handler rank must remain None" || return 1
}

release_version() {
  root="$1"
  node -p "require('$root/apps/desktop/src-tauri/tauri.conf.json').version"
}

create_dmg() {
  app="$1"
  dmg="$2"
  volume_name="$3"
  stage="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-dmg.XXXXXX")"
  trap 'rm -rf "$stage"' RETURN
  ditto "$app" "$stage/MDViewer.app"
  ln -s /Applications "$stage/Applications"
  rm -f "$dmg"
  hdiutil create -quiet -ov -format UDZO -volname "$volume_name" -srcfolder "$stage" "$dmg"
  rm -rf "$stage"
  trap - RETURN
}
