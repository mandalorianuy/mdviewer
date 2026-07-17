#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="4.1.0"
DEST="$ROOT/.cache/tessdata/$VERSION"
ENG_URL="https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata"
SPA_URL="https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/spa.traineddata"
ENG_SHA256="7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
SPA_SHA256="6f2e04d02774a18f01bed44b1111f2cd7f3ba7ac9dc4373cd3f898a40ea6b464"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

verify_language() {
  local language="$1"
  local expected="$2"
  local file="$DEST/$language.traineddata"
  test -f "$file" && test ! -L "$file" && test "$(sha256_file "$file")" = "$expected"
}

mkdir -p "$DEST"
for language in eng spa; do
  if [ "$language" = eng ]; then
    expected="$ENG_SHA256"
    url="$ENG_URL"
  else
    expected="$SPA_SHA256"
    url="$SPA_URL"
  fi
  if verify_language "$language" "$expected"; then
    continue
  fi
  temporary="$DEST/.$language.traineddata.download"
  trap 'rm -f "$temporary"' EXIT
  curl --fail --location --silent --show-error \
    --proto '=https' --tlsv1.2 \
    "$url" \
    --output "$temporary"
  test "$(sha256_file "$temporary")" = "$expected" || {
    printf 'tessdata checksum mismatch: %s\n' "$language" >&2
    exit 1
  }
  chmod 0644 "$temporary"
  mv "$temporary" "$DEST/$language.traineddata"
  trap - EXIT
done

verify_language eng "$ENG_SHA256"
verify_language spa "$SPA_SHA256"
printf '%s  %s\n%s  %s\n' \
  "$ENG_SHA256" eng.traineddata \
  "$SPA_SHA256" spa.traineddata >"$DEST/SHA256SUMS"
printf 'Verified tessdata_fast %s at %s\n' "$VERSION" "$DEST"
