#!/bin/sh
set -eu

RELEASE="chromium-7947"
ASSET="pdfium-mac-arm64.tgz"
SHA256="aa9739354fc7bc8f200f3f3c9532bd5233298203051e094820272ccd9c997a77"
URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7947/pdfium-mac-arm64.tgz"

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CACHE_ROOT="$ROOT/.cache/pdfium"
if [ "${PDFIUM_FETCH_TEST_MODE:-0}" = "1" ]; then
    URL=${PDFIUM_FETCH_TEST_URL:?PDFIUM_FETCH_TEST_URL is required in test mode}
    SHA256=${PDFIUM_FETCH_TEST_SHA256:?PDFIUM_FETCH_TEST_SHA256 is required in test mode}
    CACHE_ROOT=${PDFIUM_FETCH_TEST_CACHE_ROOT:?PDFIUM_FETCH_TEST_CACHE_ROOT is required in test mode}
fi
ARCHIVE="$CACHE_ROOT/$RELEASE-$ASSET"
INSTALL="$CACHE_ROOT/$RELEASE"
LIBRARY="$INSTALL/lib/libpdfium.dylib"
RECEIPT="$CACHE_ROOT/$RELEASE.receipt"
LOCK="$CACHE_ROOT/.fetch.lock"
OWNER_FILE="$LOCK/owner.pid"
LOCK_HELD=0
TEMP=""
NEW_LIBRARY=""
LOCK_OWNER=""

read_lock_owner() {
    owner_file="$1/owner.pid"
    if [ ! -f "$owner_file" ] || [ -L "$owner_file" ]; then
        return 1
    fi
    owner_size=$(wc -c < "$owner_file" | tr -d '[:space:]') || return 1
    case "$owner_size" in
        "" | *[!0-9]*) return 1 ;;
    esac
    if [ "$owner_size" -gt 11 ]; then
        return 1
    fi
    LOCK_OWNER=$(cat "$owner_file") || return 1
    case "$LOCK_OWNER" in
        "" | 0 | *[!0-9]*) return 1 ;;
    esac
    if [ "$LOCK_OWNER" -gt 2147483647 ]; then
        return 1
    fi
    return 0
}

lock_owner_is_verifiably_dead() {
    python3 - "$1" <<'PY'
import os
import sys

pid = int(sys.argv[1])
try:
    os.kill(pid, 0)
except ProcessLookupError:
    raise SystemExit(0)
except (PermissionError, OSError, OverflowError):
    raise SystemExit(1)
else:
    raise SystemExit(1)
PY
}

write_lock_owner() {
    owner_temp="$LOCK/.owner.pid.$$"
    if ! (umask 077 && printf '%s\n' "$$" > "$owner_temp"); then
        rm -f "$owner_temp"
        return 1
    fi
    if ! mv -f "$owner_temp" "$OWNER_FILE"; then
        rm -f "$owner_temp"
        return 1
    fi
}

acquire_lock() {
    if mkdir "$LOCK" 2>/dev/null; then
        if ! write_lock_owner; then
            rmdir "$LOCK" 2>/dev/null || true
            echo "could not record PDFium fetch lock owner" >&2
            return 1
        fi
        LOCK_HELD=1
        return 0
    fi

    if ! read_lock_owner "$LOCK"; then
        echo "malformed PDFium fetch lock owner at $LOCK; refusing recovery" >&2
        return 1
    fi
    dead_owner=$LOCK_OWNER
    if ! lock_owner_is_verifiably_dead "$dead_owner"; then
        echo "another PDFium fetch is active or unverifiable at $LOCK (pid $dead_owner)" >&2
        return 1
    fi

    stale_lock="$CACHE_ROOT/.fetch.lock.stale.$$"
    if [ -e "$stale_lock" ] || ! mv "$LOCK" "$stale_lock" 2>/dev/null; then
        echo "PDFium fetch lock changed during stale-owner recovery" >&2
        return 1
    fi
    if ! read_lock_owner "$stale_lock" || [ "$LOCK_OWNER" != "$dead_owner" ]; then
        if [ ! -e "$LOCK" ]; then
            mv "$stale_lock" "$LOCK" 2>/dev/null || true
        fi
        echo "PDFium fetch lock owner changed during stale-owner recovery" >&2
        return 1
    fi
    rm -rf "$stale_lock"

    if ! mkdir "$LOCK" 2>/dev/null; then
        echo "another PDFium fetch acquired the lock during stale-owner recovery" >&2
        return 1
    fi
    if ! write_lock_owner; then
        rmdir "$LOCK" 2>/dev/null || true
        echo "could not record PDFium fetch lock owner" >&2
        return 1
    fi
    LOCK_HELD=1
}

cleanup() {
    if [ -n "$NEW_LIBRARY" ]; then
        cleanup_library=$NEW_LIBRARY
        NEW_LIBRARY=""
        rm -f "$cleanup_library"
    fi
    if [ -n "$TEMP" ]; then
        cleanup_temp=$TEMP
        TEMP=""
        rm -rf "$cleanup_temp"
    fi
    if [ "$LOCK_HELD" = "1" ]; then
        LOCK_HELD=0
        if read_lock_owner "$LOCK" && [ "$LOCK_OWNER" = "$$" ]; then
            rm -rf "$LOCK"
        fi
    fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

mkdir -p "$CACHE_ROOT"
if ! acquire_lock; then
    exit 1
fi
TEMP=$(mktemp -d "$CACHE_ROOT/.fetch-$RELEASE.XXXXXX")

verify_archive() {
    actual=$(shasum -a 256 "$1" | awk '{print $1}')
    if [ "$actual" != "$SHA256" ]; then
        echo "PDFium archive checksum mismatch: expected $SHA256, received $actual" >&2
        return 1
    fi
    echo "$ASSET: SHA-256 OK"
}

if [ -f "$ARCHIVE" ]; then
    verify_archive "$ARCHIVE"
else
    DOWNLOAD="$TEMP/$ASSET"
    curl --fail --location --show-error --silent "$URL" --output "$DOWNLOAD"
    verify_archive "$DOWNLOAD"
    mv "$DOWNLOAD" "$ARCHIVE"
fi

EXTRACTED="$TEMP/extracted"
mkdir -p "$EXTRACTED"
python3 - "$ARCHIVE" "$EXTRACTED" <<'PY'
import os
from pathlib import Path, PurePosixPath
import shutil
import sys
import tarfile

archive = Path(sys.argv[1])
destination = Path(sys.argv[2]).resolve()

with tarfile.open(archive, "r:gz") as source:
    for member in source.getmembers():
        name = member.name
        portable = PurePosixPath(name)
        if (
            not name
            or portable.is_absolute()
            or ".." in portable.parts
            or "\\" in name
            or not (member.isdir() or member.isfile())
        ):
            raise SystemExit(f"refusing unsafe PDFium archive entry: {name!r}")

        relative = Path(*[part for part in portable.parts if part not in ("", ".")])
        target = (destination / relative).resolve()
        if target != destination and destination not in target.parents:
            raise SystemExit(f"refusing traversing PDFium archive entry: {name!r}")

        if member.isdir():
            target.mkdir(parents=True, exist_ok=True)
            continue

        target.parent.mkdir(parents=True, exist_ok=True)
        stream = source.extractfile(member)
        if stream is None:
            raise SystemExit(f"could not read PDFium archive entry: {name!r}")
        with target.open("xb") as output:
            shutil.copyfileobj(stream, output)
        os.chmod(target, member.mode & 0o777)
PY

CANDIDATES=$(find "$EXTRACTED" -type f -name libpdfium.dylib -print)
if [ -z "$CANDIDATES" ] || [ "$(printf '%s\n' "$CANDIDATES" | wc -l | tr -d ' ')" -ne 1 ]; then
    echo "unexpected PDFium archive: expected exactly one libpdfium.dylib" >&2
    exit 1
fi
CANDIDATE=$CANDIDATES

LIBRARY_SHA256=$(shasum -a 256 "$CANDIDATE" | awk '{print $1}')
RECEIPT_TEMP="$TEMP/$RELEASE.receipt"
{
    echo "release=$RELEASE"
    echo "asset=$ASSET"
    echo "archive_sha256=$SHA256"
    echo "library_sha256=$LIBRARY_SHA256"
} > "$RECEIPT_TEMP"

if [ -f "$LIBRARY" ] && cmp -s "$CANDIDATE" "$LIBRARY"; then
    mv -f "$RECEIPT_TEMP" "$RECEIPT"
    echo "Reusing verified PDFium installation at $LIBRARY"
    exit 0
fi

mkdir -p "$INSTALL/lib"
NEW_LIBRARY="$INSTALL/lib/.libpdfium.dylib.new.$$"
cp "$CANDIDATE" "$NEW_LIBRARY"
chmod 755 "$NEW_LIBRARY"
mv -f "$NEW_LIBRARY" "$LIBRARY"
NEW_LIBRARY=""
mv -f "$RECEIPT_TEMP" "$RECEIPT"
echo "Installed verified PDFium at $LIBRARY"
