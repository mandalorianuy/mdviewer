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
LOCK="$CACHE_ROOT/.fetch.lock"
LOCK_HELD=0
TEMP=""
NEW_LIBRARY=""

cleanup() {
    if [ -n "$NEW_LIBRARY" ]; then
        rm -f "$NEW_LIBRARY"
    fi
    if [ -n "$TEMP" ]; then
        rm -rf "$TEMP"
    fi
    if [ "$LOCK_HELD" = "1" ]; then
        rm -rf "$LOCK"
    fi
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$CACHE_ROOT"
if ! mkdir "$LOCK" 2>/dev/null; then
    echo "another PDFium fetch is active at $LOCK" >&2
    exit 1
fi
LOCK_HELD=1
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

if [ -f "$LIBRARY" ] && cmp -s "$CANDIDATE" "$LIBRARY"; then
    echo "Reusing verified PDFium installation at $LIBRARY"
    exit 0
fi

mkdir -p "$INSTALL/lib"
NEW_LIBRARY="$INSTALL/lib/.libpdfium.dylib.new.$$"
cp "$CANDIDATE" "$NEW_LIBRARY"
chmod 755 "$NEW_LIBRARY"
mv -f "$NEW_LIBRARY" "$LIBRARY"
NEW_LIBRARY=""
echo "Installed verified PDFium at $LIBRARY"
