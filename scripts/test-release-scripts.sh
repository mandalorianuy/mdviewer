#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMON="$ROOT/scripts/release-common.sh"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

expect_failure() {
  label="$1"
  shift
  output="$(mktemp)"
  if "$@" >"$output" 2>&1; then
    rm -f "$output"
    fail "$label unexpectedly succeeded"
  fi
  cat "$output"
  rm -f "$output"
}

test -f "$COMMON" || fail "release-common.sh is missing"
# shellcheck source=release-common.sh
. "$COMMON"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-release-tests.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

printf 'not a Mach-O binary\n' >"$tmp/not-arm64"
expect_failure "non-arm64 artifact" require_arm64_file "$tmp/not-arm64"
printf 'int main(void) { return 0; }\n' | clang -arch arm64 -x c - -o "$tmp/arm64"
require_arm64_file "$tmp/arm64"
expect_failure "universal artifact" require_arm64_file /usr/bin/true

mkdir -p "$tmp/pdfium/.cache/pdfium/chromium-7947/lib"
cp "$ROOT/.cache/pdfium/chromium-7947-pdfium-mac-arm64.tgz" \
  "$tmp/pdfium/.cache/pdfium/chromium-7947-pdfium-mac-arm64.tgz"
cp "$ROOT/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" \
  "$tmp/pdfium/.cache/pdfium/chromium-7947/lib/libpdfium.dylib"
expect_failure "missing PDFium receipt" verify_pdfium_receipt "$tmp/pdfium"

cp "$ROOT/.cache/pdfium/chromium-7947.receipt" \
  "$tmp/pdfium/.cache/pdfium/chromium-7947.receipt"
verify_pdfium_receipt "$tmp/pdfium"
printf 'tampered\n' >>"$tmp/pdfium/.cache/pdfium/chromium-7947/lib/libpdfium.dylib"
expect_failure "tampered PDFium library" verify_pdfium_receipt "$tmp/pdfium"

mkdir -p "$tmp/hardcoded/legacy/macos-swift/scripts"
cat >"$tmp/hardcoded/legacy/macos-swift/scripts/bad.sh" <<'EOF'
AUTH_KEY_ID="J3JJ2WXQ5S"
AUTH_ISSUER_ID="c9f7eed4-57f2-4c22-8efa-8e2cf829a79e"
EOF
expect_failure "hardcoded App Store Connect identifiers" reject_hardcoded_apple_ids "$tmp/hardcoded"
cat >"$tmp/hardcoded/legacy/macos-swift/scripts/bad.sh" <<'EOF'
: "${APPLE_API_KEY:?required}"
: "${APPLE_API_ISSUER:?required}"
EOF
reject_hardcoded_apple_ids "$tmp/hardcoded"
cat >"$tmp/hardcoded/legacy/macos-swift/scripts/bad.sh" <<'EOF'
ASC_KEY_ID='ABCDEFGHIJ'
ASC_ISSUER_ID='11111111-2222-3333-4444-555555555555'
EOF
expect_failure "single-quoted App Store Connect identifiers" reject_hardcoded_apple_ids "$tmp/hardcoded"

mkdir -p "$tmp/config/apps/desktop/src-tauri"
cat >"$tmp/config/apps/desktop/src-tauri/tauri.macos.conf.json" <<'EOF'
{"bundle":{"fileAssociations":[]}}
EOF
expect_failure "incorrect native PDF alias configuration" verify_macos_print_contract "$tmp/config"
cat >"$tmp/config/apps/desktop/src-tauri/tauri.macos.conf.json" <<'EOF'
{"bundle":{"fileAssociations":[{"ext":["pdf"],"contentTypes":["com.adobe.pdf"],"role":"Viewer","rank":"None"}],"resources":{"../../../.cache/pdfium/chromium-7947/lib/libpdfium.dylib":"lib/libpdfium.dylib"}}}
EOF
verify_macos_print_contract "$tmp/config"

mkdir -p "$tmp/repo"
git -C "$tmp/repo" init -q
git -C "$tmp/repo" config user.email test@example.invalid
git -C "$tmp/repo" config user.name Test
printf 'lock\n' >"$tmp/repo/Cargo.lock"
printf 'lock\n' >"$tmp/repo/package-lock.json"
git -C "$tmp/repo" add Cargo.lock package-lock.json
git -C "$tmp/repo" commit -qm initial
verify_clean_release_tree "$tmp/repo"
printf 'changed\n' >>"$tmp/repo/Cargo.lock"
expect_failure "modified lockfile" verify_clean_release_tree "$tmp/repo"
git -C "$tmp/repo" restore Cargo.lock
printf 'dirty\n' >"$tmp/repo/untracked"
expect_failure "dirty tree" verify_clean_release_tree "$tmp/repo"

expect_failure "unsigned app" verify_developer_id_app /usr/bin/true

test -f "$ROOT/.github/workflows/ci.yml" || fail "CI workflow is missing"
test -f "$ROOT/.github/workflows/release-macos.yml" || fail "macOS release workflow is missing"
grep -q 'actions/checkout@v6' "$ROOT/.github/workflows/ci.yml" || fail "CI must use checkout v6"
grep -q 'actions/setup-node@v6' "$ROOT/.github/workflows/ci.yml" || fail "CI must use setup-node v6"
grep -q 'ubuntu-22.04' "$ROOT/.github/workflows/ci.yml" || fail "Linux CI lane is missing"
grep -q 'windows-latest' "$ROOT/.github/workflows/ci.yml" || fail "Windows CI lane is missing"
grep -q 'macos-15' "$ROOT/.github/workflows/ci.yml" || fail "Apple Silicon CI lane is missing"
grep -q -- '--no-bundle' "$ROOT/.github/workflows/ci.yml" || fail "portable Tauri smoke is missing"
grep -q 'aarch64-apple-darwin' "$ROOT/.github/workflows/release-macos.yml" || fail "arm64 release target is missing"
if grep -Eq 'x86_64-apple-darwin|universal-apple-darwin' "$ROOT/.github/workflows/release-macos.yml"; then
  fail "release workflow must not build Intel or universal artifacts"
fi

for data in \
  "$ROOT/config/audit/rust-advisory-allowlist.txt" \
  "$ROOT/config/audit/npm-advisory-allowlist.json"; do
  test -f "$data" || fail "audit allowlist data is missing: $data"
done

for script in audit.sh package-macos-arm64.sh notarize-macos.sh verify-release.sh; do
  test -x "$ROOT/scripts/$script" || fail "$script is missing or not executable"
done
grep -q 'cargo-audit --version' "$ROOT/scripts/audit.sh" ||
  fail "audit version gate must invoke the binary directly"
if grep -q 'cargo audit --version' "$ROOT/scripts/audit.sh"; then
  fail "cargo audit subcommand duplicates its executable name in version output"
fi
grep -q -- '--deny warnings' "$ROOT/scripts/audit.sh" ||
  fail "RustSec warnings must fail closed unless explicitly allowlisted"
grep -q 'auditReportVersion !== 2' "$ROOT/scripts/audit.sh" ||
  fail "npm audit parser must reject error payloads instead of treating them as empty reports"
grep -q 'fetch-pdfium.sh' "$ROOT/scripts/verify-workspace.sh" ||
  fail "workspace gate must provision pinned PDFium on Apple Silicon"
grep -q -- '--exclude mdconvert-pdf' "$ROOT/scripts/verify-workspace.sh" ||
  fail "workspace gate must keep non-macOS lanes independent of an unpublished runtime"
grep -q 'publishable: false' "$ROOT/scripts/package-macos-arm64.sh" ||
  fail "a signed but unnotarized package receipt must not be publishable"
grep -q 'receipt.publishable = true' "$ROOT/scripts/notarize-macos.sh" ||
  fail "only completed notarization may mark a package publishable"
grep -q 'signed_application_uses_its_bundled_pdfium_without_environment_configuration' \
  "$ROOT/scripts/verify-release.sh" ||
  fail "production verification must prove nested PDFium signature and packaged conversion"

printf 'release script contract tests passed\n'
