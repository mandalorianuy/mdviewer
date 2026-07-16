#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

"$ROOT/tests/parity/verify-parity.test.sh"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
if [ "$(uname -s)" = "Darwin" ] && [ "$(uname -m)" = "arm64" ]; then
  "$ROOT/scripts/fetch-pdfium.sh"
  # shellcheck source=release-common.sh
  . "$ROOT/scripts/release-common.sh"
  verify_pdfium_receipt "$ROOT"
  export PDFIUM_DYNAMIC_LIB_PATH="$ROOT/.cache/pdfium/chromium-7947/lib/libpdfium.dylib"
  cargo test --workspace
else
  cargo test --workspace --exclude mdconvert-pdf
  cargo test -p mdconvert-pdf --test pdf_conversion
fi
npm run check
npm test -- --run
npm run build
