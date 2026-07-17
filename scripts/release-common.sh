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
  ['key ID assignment', /\b(?:AUTH_KEY_ID|ASC_KEY_ID|APPLE_API_KEY(?:_ID)?)\b["']?\s*(?:=|:)\s*["']?[A-Z0-9]{10}["']?/],
  ['issuer ID assignment', /\b(?:AUTH_ISSUER_ID|ASC_ISSUER_ID|APPLE_API_ISSUER)\b["']?\s*(?:=|:)\s*["']?[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}["']?/],
  ['key ID default', /(?:AUTH_KEY_ID|ASC_KEY_ID|APPLE_API_KEY(?:_ID)?)[^\n]{0,120}:-[A-Z0-9]{10}/],
  ['issuer ID default', /(?:AUTH_ISSUER_ID|ASC_ISSUER_ID|APPLE_API_ISSUER)[^\n]{0,160}:-[0-9a-fA-F-]{36}/],
  ['key ID CLI argument', /--key-id(?:=|\s+)["']?[A-Z0-9]{10}["']?/],
  ['issuer ID CLI argument', /--issuer(?:=|\s+)["']?[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}["']?/],
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

verify_production_signing_identity() {
  local identity="${1:-}"
  local identities

  case "$identity" in
    'Developer ID Application: '?*) ;;
    *) release_die "CODESIGN_IDENTITY must name a Developer ID Application identity"; return 1 ;;
  esac
  case "$identity" in
    *$'\n'*|*$'\r'*|*'"'*)
      release_die "CODESIGN_IDENTITY contains invalid characters"
      return 1
      ;;
  esac
  require_command security || return 1
  identities="$(security find-identity -v -p codesigning 2>/dev/null)" || {
    release_die "available code-signing identities could not be inspected"
    return 1
  }
  printf '%s\n' "$identities" | grep -Fq "\"$identity\"" || {
    release_die "CODESIGN_IDENTITY is unavailable in the active keychains"
    return 1
  }
}

verify_notarization_credentials() {
  local identity="${1:-}"
  local key_id="${2:-}"
  local issuer="${3:-}"
  local key_path="${4:-}"

  verify_production_signing_identity "$identity" || return 1
  printf '%s\n' "$key_id" | grep -Eq '^[A-Z0-9]{10}$' || {
    release_die "APPLE_API_KEY must be exactly 10 uppercase alphanumeric characters"
    return 1
  }
  printf '%s\n' "$issuer" |
    grep -Eq '^[0-9A-Fa-f]{8}(-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}$' || {
      release_die "APPLE_API_ISSUER must be a UUID"
      return 1
    }
  test -n "$key_path" && test -f "$key_path" && test -r "$key_path" && test ! -L "$key_path" || {
    release_die "APPLE_API_KEY_PATH must be a readable regular non-symlink file"
    return 1
  }
  if ! grep -q '^-----BEGIN PRIVATE KEY-----$' "$key_path" ||
     ! grep -q '^-----END PRIVATE KEY-----$' "$key_path"; then
    release_die "APPLE_API_KEY_PATH does not contain a PEM private key"
    return 1
  fi
}

codesign_details_have_hardened_runtime() {
  printf '%s\n' "$1" | grep -Eq '(^|[[:space:]])flags=[^[:space:]]*runtime'
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
  codesign_details_have_hardened_runtime "$details" ||
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
  /usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes:0:CFBundleTypeName' "$app/Contents/Info.plist" |
    grep -qx 'PDF document from macOS Print' || release_die "unexpected PDF document type name" || return 1
  /usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes:0:CFBundleTypeExtensions:0' "$app/Contents/Info.plist" |
    grep -qx 'pdf' || release_die "PDF document extension metadata is missing" || return 1
  /usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes:0:LSItemContentTypes:0' "$app/Contents/Info.plist" |
    grep -qx 'com.adobe.pdf' || release_die "PDF content type metadata is missing" || return 1
  /usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes:0:CFBundleTypeRole' "$app/Contents/Info.plist" |
    grep -qx 'Viewer' || release_die "PDF document role must remain Viewer" || return 1
}

verify_package_receipt() {
  local root="$1"
  local receipt="$2"
  local expected_mode="$3"
  local app="$4"
  local dmg="$5"
  local expected_notarized="$6"
  local expected_publishable="$7"
  local head_commit

  test -f "$receipt" && test ! -L "$receipt" ||
    release_die "package receipt is missing or is a symlink" || return 1
  test -d "$app" || release_die "application bundle is missing: $app" || return 1
  test -f "$dmg" && test ! -L "$dmg" ||
    release_die "DMG is missing or is a symlink: $dmg" || return 1
  head_commit="$(git -C "$root" rev-parse HEAD 2>/dev/null)" ||
    release_die "release root has no Git HEAD" || return 1

  node - "$receipt" "$expected_mode" "$expected_notarized" "$expected_publishable" \
    "$head_commit" "$app/Contents/MacOS/mdviewer-desktop" \
    "$app/Contents/Resources/lib/libpdfium.dylib" "$dmg" <<'NODE' || {
const crypto = require('node:crypto');
const fs = require('node:fs');
const [receiptPath, expectedMode, expectedNotarized, expectedPublishable, headCommit, executable, pdfium, dmg] = process.argv.slice(2);
const fail = (message) => { console.error(`package receipt rejected: ${message}`); process.exit(1); };
let receipt;
try { receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8')); } catch { fail('invalid JSON'); }
const expectedBoolean = (value) => value === 'true';
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
if (receipt.schemaVersion !== 1) fail('unsupported schemaVersion');
if (receipt.mode !== expectedMode) fail('mode mismatch');
if (receipt.target !== 'aarch64-apple-darwin') fail('target mismatch');
if (receipt.signed !== (expectedMode === 'signed')) fail('signed claim mismatch');
if (receipt.notarized !== expectedBoolean(expectedNotarized)) fail('notarized claim mismatch');
if (receipt.publishable !== expectedBoolean(expectedPublishable)) fail('publishable claim mismatch');
if (receipt.publishable && (!receipt.signed || !receipt.notarized)) fail('publishable state lacks prerequisites');
if (receipt.commit !== headCommit) fail('commit does not match Git HEAD');
if (receipt.artifacts?.executableSha256 !== sha256(executable)) fail('executable checksum mismatch');
if (receipt.artifacts?.pdfiumSha256 !== sha256(pdfium)) fail('PDFium checksum mismatch');
if (receipt.artifacts?.dmgSha256 !== sha256(dmg)) fail('DMG checksum mismatch');
NODE
    release_die "package receipt provenance or state validation failed"
    return 1
  }
}

verify_mounted_release_contents() {
  local receipt="$1"
  local outer_app="$2"
  local mounted_app="$3"
  local applications_link="$4"

  test -L "$applications_link" || release_die "DMG does not contain the Applications link" || return 1
  [ "$(readlink "$applications_link")" = "/Applications" ] ||
    release_die "DMG Applications link must point exactly to /Applications" || return 1
  verify_unsigned_app "$outer_app" || return 1
  verify_unsigned_app "$mounted_app" || return 1

  node - "$receipt" "$outer_app" "$mounted_app" <<'NODE' || {
const crypto = require('node:crypto');
const fs = require('node:fs');
const [receiptPath, outerApp, mountedApp] = process.argv.slice(2);
const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
for (const [relative, key] of [
  ['Contents/MacOS/mdviewer-desktop', 'executableSha256'],
  ['Contents/Resources/lib/libpdfium.dylib', 'pdfiumSha256'],
]) {
  const outer = sha256(`${outerApp}/${relative}`);
  const mounted = sha256(`${mountedApp}/${relative}`);
  if (outer !== mounted || mounted !== receipt.artifacts?.[key]) process.exit(1);
}
NODE
    release_die "mounted DMG application does not match the exterior app and receipt"
    return 1
  }
}

atomic_update_package_receipt() {
  local receipt="$1"
  local operation="$2"
  local dmg="${3:-}"
  local root="${4:-}"
  local app="${5:-}"
  node - "$receipt" "$operation" "$dmg" "$root" "$app" <<'NODE' || {
const crypto = require('node:crypto');
const childProcess = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const [receiptPath, operation, dmgPath, root, app] = process.argv.slice(2);
const directory = path.dirname(receiptPath);
const temporary = path.join(directory, `.${path.basename(receiptPath)}.${process.pid}.${crypto.randomBytes(6).toString('hex')}.tmp`);
const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
if (receipt.schemaVersion !== 1 || receipt.mode !== 'signed' || !receipt.signed) process.exit(1);
if (!root || !app || !dmgPath) process.exit(1);
const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const head = childProcess.execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
if (receipt.commit !== head) process.exit(1);
if (receipt.artifacts?.executableSha256 !== sha256(`${app}/Contents/MacOS/mdviewer-desktop`)) process.exit(1);
if (receipt.artifacts?.pdfiumSha256 !== sha256(`${app}/Contents/Resources/lib/libpdfium.dylib`)) process.exit(1);
if (operation === 'notarized') {
  if (receipt.notarized || receipt.publishable) process.exit(1);
  receipt.artifacts.dmgSha256 = sha256(dmgPath);
  receipt.notarized = true;
  receipt.publishable = false;
} else if (operation === 'publishable') {
  if (!receipt.notarized || receipt.publishable) process.exit(1);
  if (receipt.artifacts?.dmgSha256 !== sha256(dmgPath)) process.exit(1);
  receipt.publishable = true;
} else {
  process.exit(1);
}
const data = `${JSON.stringify(receipt, null, 2)}\n`;
let descriptor;
try {
  descriptor = fs.openSync(temporary, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL, 0o600);
  fs.writeFileSync(descriptor, data);
  fs.fsyncSync(descriptor);
  fs.closeSync(descriptor);
  descriptor = undefined;
  fs.renameSync(temporary, receiptPath);
  const directoryDescriptor = fs.openSync(directory, fs.constants.O_RDONLY);
  fs.fsyncSync(directoryDescriptor);
  fs.closeSync(directoryDescriptor);
} catch (error) {
  if (descriptor !== undefined) fs.closeSync(descriptor);
  try { fs.unlinkSync(temporary); } catch {}
  throw error;
}
NODE
    release_die "atomic package receipt transition failed: $operation"
    return 1
  }
}

mark_package_receipt_notarized() {
  atomic_update_package_receipt "$1" notarized "$2" "$3" "$4"
}

mark_package_receipt_publishable() {
  atomic_update_package_receipt "$1" publishable "$2" "$3" "$4"
}

verify_then_publish_release() {
  local root="$1"
  local receipt="$2"
  local app="$3"
  local dmg="$4"
  local gate_function="$5"

  verify_package_receipt "$root" "$receipt" signed "$app" "$dmg" true false || return 1
  type "$gate_function" >/dev/null 2>&1 || release_die "release gate function is unavailable" || return 1
  "$gate_function" "$root" "$receipt" "$app" "$dmg" || {
    release_die "signed release verification gate failed"
    return 1
  }
  verify_package_receipt "$root" "$receipt" signed "$app" "$dmg" true false || return 1
  mark_package_receipt_publishable "$receipt" "$dmg" "$root" "$app" || return 1
  verify_package_receipt "$root" "$receipt" signed "$app" "$dmg" true true
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
