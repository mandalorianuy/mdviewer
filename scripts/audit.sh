#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST_ALLOWLIST="$ROOT/config/audit/rust-advisory-allowlist.txt"
NPM_ALLOWLIST="$ROOT/config/audit/npm-advisory-allowlist.json"

test -f "$RUST_ALLOWLIST" || { echo "Rust advisory allowlist is missing" >&2; exit 1; }
test -f "$NPM_ALLOWLIST" || { echo "npm advisory allowlist is missing" >&2; exit 1; }

if ! command -v cargo-audit >/dev/null 2>&1; then
  echo "cargo-audit 0.22.2 is required; install it with: cargo install cargo-audit --locked --version 0.22.2" >&2
  exit 2
fi
if [ "$(cargo-audit --version)" != "cargo-audit 0.22.2" ]; then
  echo "cargo-audit 0.22.2 is required for a reproducible audit" >&2
  exit 2
fi

rust_args=()
while IFS= read -r advisory; do
  case "$advisory" in
    ''|'#'*) continue ;;
  esac
  id="${advisory%%[[:space:]]*}"
  rationale="${advisory#"$id"}"
  case "$id" in
    RUSTSEC-[0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9]) ;;
    *) echo "Invalid Rust advisory allowlist entry: $id" >&2; exit 1 ;;
  esac
  if [ -z "${rationale//[[:space:]]/}" ]; then
    echo "Rust advisory allowlist entry lacks a rationale: $id" >&2
    exit 1
  fi
  rust_args+=(--ignore "$id")
done <"$RUST_ALLOWLIST"

printf '==> RustSec audit\n'
cargo audit --file "$ROOT/Cargo.lock" --deny warnings "${rust_args[@]}"

printf '==> npm audit\n'
npm_report="$(mktemp "${TMPDIR:-/tmp}/mdviewer-npm-audit.XXXXXX")"
trap 'rm -f "$npm_report"' EXIT
set +e
(cd "$ROOT" && npm audit --json) >"$npm_report"
npm_status=$?
set -e

node - "$npm_report" "$NPM_ALLOWLIST" <<'NODE'
const fs = require('node:fs');
const report = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const data = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
if (
  report.auditReportVersion !== 2 ||
  !report.vulnerabilities ||
  typeof report.vulnerabilities !== 'object' ||
  Array.isArray(report.vulnerabilities)
) {
  throw new Error('npm audit did not return a version 2 vulnerability report');
}
if (!data || typeof data.allowedAdvisories !== 'object' || Array.isArray(data.allowedAdvisories)) {
  throw new Error('npm allowlist must contain an allowedAdvisories object');
}
for (const [id, rationale] of Object.entries(data.allowedAdvisories)) {
  if (!/^\d+$/.test(id) || typeof rationale !== 'string' || !rationale.trim()) {
    throw new Error(`invalid npm advisory allowlist entry: ${id}`);
  }
}

const rejected = [];
for (const [packageName, vulnerability] of Object.entries(report.vulnerabilities ?? {})) {
  const sources = (vulnerability.via ?? [])
    .filter((entry) => typeof entry === 'object' && entry !== null && Number.isInteger(entry.source))
    .map((entry) => String(entry.source));
  if (sources.length === 0 || sources.some((source) => !(source in data.allowedAdvisories))) {
    rejected.push(`${packageName}: ${sources.length ? sources.join(',') : 'unidentified advisory'}`);
  }
}
if (rejected.length) {
  console.error(`npm audit found non-allowlisted vulnerabilities:\n${rejected.join('\n')}`);
  process.exit(1);
}
NODE

printf 'dependency audits passed\n'
