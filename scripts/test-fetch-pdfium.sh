#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FETCHER="$ROOT/scripts/fetch-pdfium.sh"
TEMP=$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-fetch-pdfium.XXXXXX")
LIVE_HOLDER=""
STALE_HOLDER=""
cleanup() {
    for holder in "$LIVE_HOLDER" "$STALE_HOLDER"; do
        if [ -n "$holder" ] && kill -0 "$holder" 2>/dev/null; then
            kill "$holder" 2>/dev/null || true
            wait "$holder" 2>/dev/null || true
        fi
    done
    rm -rf "$TEMP"
}
trap cleanup EXIT HUP INT TERM

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
grep -q "malformed PDFium fetch lock owner" "$TEMP/lock.out"

INVALID_PID_CACHE="$TEMP/cache-invalid-pid-lock"
mkdir -p "$INVALID_PID_CACHE/.fetch.lock"
printf '%s\n' "999999999999999999999999" > "$INVALID_PID_CACHE/.fetch.lock/owner.pid"
if run_fetch "$VALID" "$VALID_SHA" "$INVALID_PID_CACHE" >"$TEMP/invalid-pid.out" 2>&1; then
    echo "invalid lock owner pid unexpectedly reclaimed" >&2
    exit 1
fi
grep -q "malformed PDFium fetch lock owner" "$TEMP/invalid-pid.out"

LIVE_CACHE="$TEMP/cache-live-lock"
LIVE_READY="$TEMP/live-lock-ready"
mkdir -p "$LIVE_CACHE"
sh -c '
    lock=$1/.fetch.lock
    mkdir "$lock"
    owner_tmp="$lock/.owner.pid.$$"
    printf "%s\n" "$$" > "$owner_tmp"
    mv "$owner_tmp" "$lock/owner.pid"
    : > "$2"
    exec sleep 60
' sh "$LIVE_CACHE" "$LIVE_READY" &
LIVE_HOLDER=$!
while [ ! -f "$LIVE_READY" ]; do
    kill -0 "$LIVE_HOLDER"
    sleep 0.01
done
if run_fetch "$VALID" "$VALID_SHA" "$LIVE_CACHE" >"$TEMP/live-lock.out" 2>&1; then
    echo "live lock owner unexpectedly reclaimed" >&2
    exit 1
fi
grep -q "another PDFium fetch is active" "$TEMP/live-lock.out"
kill "$LIVE_HOLDER"
wait "$LIVE_HOLDER" 2>/dev/null || true
LIVE_HOLDER=""

STALE_CACHE="$TEMP/cache-stale-lock"
STALE_READY="$TEMP/stale-lock-ready"
mkdir -p "$STALE_CACHE"
sh -c '
    lock=$1/.fetch.lock
    mkdir "$lock"
    owner_tmp="$lock/.owner.pid.$$"
    printf "%s\n" "$$" > "$owner_tmp"
    mv "$owner_tmp" "$lock/owner.pid"
    : > "$2"
    exec sleep 60
' sh "$STALE_CACHE" "$STALE_READY" &
STALE_HOLDER=$!
while [ ! -f "$STALE_READY" ]; do
    kill -0 "$STALE_HOLDER"
    sleep 0.01
done
kill "$STALE_HOLDER"
wait "$STALE_HOLDER" 2>/dev/null || true
STALE_HOLDER=""
STALE_RESULT=$(run_fetch "$VALID" "$VALID_SHA" "$STALE_CACHE")
printf '%s\n' "$STALE_RESULT" | grep -q "Installed verified PDFium"
printf 'verified-pdfium\n' | cmp - "$STALE_CACHE/chromium-7947/lib/libpdfium.dylib"

echo "fetch-pdfium shell regressions: PASS"
