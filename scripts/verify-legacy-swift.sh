#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
swift test --package-path "$ROOT/legacy/macos-swift"
