#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FETCHER="$ROOT/scripts/fetch-pdfium.sh"
TEMP=$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-fetch-pdfium.XXXXXX")
trap 'rm -rf "$TEMP"' EXIT HUP INT TERM

make_archive() {
    python3 - "$1" "$2" <<'PY'
import io
import sys
import tarfile

output, kind = sys.argv[1:]
with tarfile.open(output, "w:gz") as archive:
    entries = {
        "valid": [("bundle/lib/libpdfium.dylib", b"verified-pdfium\n")],
        "unsafe": [("../escape", b"escaped\n")],
        "duplicate": [
            ("a/libpdfium.dylib", b"first\n"),
            ("b/libpdfium.dylib", b"second\n"),
        ],
    }[kind]
    for name, data in entries:
        member = tarfile.TarInfo(name)
        member.size = len(data)
        member.mode = 0o755
        member.mtime = 0
        archive.addfile(member, io.BytesIO(data))
PY
}

sha256() {
    shasum -a 256 "$1" | awk '{print $1}'
}

run_fetch() {
    PDFIUM_FETCH_TEST_MODE=1 \
    PDFIUM_FETCH_TEST_URL="file://$1" \
    PDFIUM_FETCH_TEST_SHA256="$2" \
    PDFIUM_FETCH_TEST_CACHE_ROOT="$3" \
        "$FETCHER"
}

VALID="$TEMP/valid.tgz"
make_archive "$VALID" valid
VALID_SHA=$(sha256 "$VALID")
CACHE="$TEMP/cache-valid"

FIRST=$(run_fetch "$VALID" "$VALID_SHA" "$CACHE")
printf '%s\n' "$FIRST" | grep -q "Installed verified PDFium"
printf 'verified-pdfium\n' | cmp - "$CACHE/chromium-7947/lib/libpdfium.dylib"

SECOND=$(run_fetch "$VALID" "$VALID_SHA" "$CACHE")
printf '%s\n' "$SECOND" | grep -q "Reusing verified PDFium installation"

printf 'tampered-install\n' > "$CACHE/chromium-7947/lib/libpdfium.dylib"
run_fetch "$VALID" "$VALID_SHA" "$CACHE" >/dev/null
printf 'verified-pdfium\n' | cmp - "$CACHE/chromium-7947/lib/libpdfium.dylib"

ARCHIVE_CACHE="$CACHE/chromium-7947-pdfium-mac-arm64.tgz"
printf 'tampered-archive\n' >> "$ARCHIVE_CACHE"
BEFORE=$(sha256 "$CACHE/chromium-7947/lib/libpdfium.dylib")
if run_fetch "$VALID" "$VALID_SHA" "$CACHE" >"$TEMP/tampered.out" 2>&1; then
    echo "tampered cached archive unexpectedly succeeded" >&2
    exit 1
fi
grep -q "checksum mismatch" "$TEMP/tampered.out"
[ "$(sha256 "$CACHE/chromium-7947/lib/libpdfium.dylib")" = "$BEFORE" ]

UNSAFE="$TEMP/unsafe.tgz"
make_archive "$UNSAFE" unsafe
UNSAFE_CACHE="$TEMP/cache-unsafe"
if run_fetch "$UNSAFE" "$(sha256 "$UNSAFE")" "$UNSAFE_CACHE" >"$TEMP/unsafe.out" 2>&1; then
    echo "unsafe archive unexpectedly succeeded" >&2
    exit 1
fi
grep -q "refusing unsafe PDFium archive entry" "$TEMP/unsafe.out"
[ ! -e "$TEMP/escape" ]

DUPLICATE="$TEMP/duplicate.tgz"
make_archive "$DUPLICATE" duplicate
DUPLICATE_CACHE="$TEMP/cache-duplicate"
if run_fetch "$DUPLICATE" "$(sha256 "$DUPLICATE")" "$DUPLICATE_CACHE" >"$TEMP/duplicate.out" 2>&1; then
    echo "duplicate dylib archive unexpectedly succeeded" >&2
    exit 1
fi
grep -q "expected exactly one libpdfium.dylib" "$TEMP/duplicate.out"

LOCK_CACHE="$TEMP/cache-lock"
mkdir -p "$LOCK_CACHE/.fetch.lock"
if run_fetch "$VALID" "$VALID_SHA" "$LOCK_CACHE" >"$TEMP/lock.out" 2>&1; then
    echo "contended fetch unexpectedly succeeded" >&2
    exit 1
fi
grep -q "another PDFium fetch is active" "$TEMP/lock.out"

echo "fetch-pdfium shell regressions: PASS"
