#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERIFY="$ROOT/scripts/verify-parity.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/mdviewer-parity-tests.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

expect_failure() {
  label="$1"
  shift
  output="$TMP/failure.out"
  if "$@" >"$output" 2>&1; then
    fail "$label unexpectedly succeeded"
  fi
}

printf 'selector fixture\n' >"$TMP/evidence.txt"

write_manifest() {
  destination="$1"
  behavior_status="$2"
  manual_status="$3"
  evidence_path="$4"
  if [ "$behavior_status" = "pending" ]; then
    evidence="{\"kind\":\"missing_implementation\",\"blocker\":\"fixture blocker\"}"
  else
    evidence="{\"kind\":\"automated_test\",\"suite\":\"fixture\",\"path\":\"$evidence_path\",\"selector\":\"selector fixture\"}"
  fi
  cat >"$destination" <<EOF
{
  "schemaVersion": 1,
  "scope": "v1",
  "automatedSuites": [
    {"id": "fixture", "command": ["true"]}
  ],
  "behaviors": [
    {
      "id": "viewer.open",
      "area": "viewer",
      "requirement": "Open Markdown",
      "disposition": "required",
      "status": "$behavior_status",
      "evidence": $evidence
    },
    {
      "id": "ocr",
      "area": "exclusion",
      "requirement": "OCR is deferred",
      "disposition": "excluded",
      "status": "excluded",
      "evidence": {
        "kind": "approved_exclusion",
        "path": "docs/superpowers/specs/2026-07-15-cross-platform-save-as-markdown-design.md",
        "selector": "OCR y conversión fiable"
      },
      "target": "v1.1"
    }
  ],
  "manualAcceptance": {
    "requiredChecks": ["reading_order", "cleanup"],
    "rows": [
      {
        "id": "safari",
        "application": "Safari",
        "bundleId": "com.apple.Safari",
        "availability": "available",
        "fixtures": ["tests/fixtures/html/semantic.html"],
        "status": "$manual_status",
        "evidence": null
      }
    ]
  }
}
EOF
}

write_manifest "$TMP/pending.json" pending pending "$TMP/evidence.txt"
"$VERIFY" --manifest "$TMP/pending.json" --automated
expect_failure "strict pending gate" "$VERIFY" --manifest "$TMP/pending.json"

write_manifest "$TMP/complete.json" pass pass "$TMP/evidence.txt"
fixture_sha="$(shasum -a 256 "$ROOT/tests/fixtures/html/semantic.html" | awk '{print $1}')"
cat >"$TMP/manual.json" <<EOF
{
  "schemaVersion": 1,
  "rowId": "safari",
  "result": "pass",
  "application": "Safari",
  "bundleId": "com.apple.Safari",
  "applicationVersion": "fixture",
  "generatedAt": "2026-07-16T15:00:00Z",
  "environment": {"platform": "macOS", "osVersion": "fixture", "architecture": "arm64"},
  "fixtureSha256": {"tests/fixtures/html/semantic.html": "$fixture_sha"},
  "output": {"markdownSha256": "0000000000000000000000000000000000000000000000000000000000000000", "bytes": 1},
  "warnings": [],
  "checks": {"reading_order": "pass", "cleanup": "pass"}
}
EOF
node - "$TMP/complete.json" "$TMP/manual.json" <<'NODE'
const fs = require('node:fs');
const [manifestPath, evidencePath] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
manifest.manualAcceptance.rows[0].evidence = evidencePath;
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
"$VERIFY" --manifest "$TMP/complete.json"

write_manifest "$TMP/missing-selector.json" pass pass "$TMP/evidence.txt"
node - "$TMP/missing-selector.json" "$TMP/manual.json" <<'NODE'
const fs = require('node:fs');
const [manifestPath, evidencePath] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
manifest.behaviors[0].evidence.selector = 'not present';
manifest.manualAcceptance.rows[0].evidence = evidencePath;
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
expect_failure "missing automated selector" "$VERIFY" --manifest "$TMP/missing-selector.json"

write_manifest "$TMP/bad-exclusion.json" pass pass "$TMP/evidence.txt"
node - "$TMP/bad-exclusion.json" "$TMP/manual.json" <<'NODE'
const fs = require('node:fs');
const [manifestPath, evidencePath] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
manifest.behaviors[1].target = 'v1';
manifest.manualAcceptance.rows[0].evidence = evidencePath;
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
expect_failure "exclusion without a future target" "$VERIFY" --manifest "$TMP/bad-exclusion.json"

write_manifest "$TMP/incomplete-manual.json" pass pass "$TMP/evidence.txt"
cp "$TMP/manual.json" "$TMP/incomplete-evidence.json"
node - "$TMP/incomplete-evidence.json" <<'NODE'
const fs = require('node:fs');
const evidencePath = process.argv[2];
const receipt = JSON.parse(fs.readFileSync(evidencePath, 'utf8'));
delete receipt.checks.cleanup;
fs.writeFileSync(evidencePath, `${JSON.stringify(receipt, null, 2)}\n`);
NODE
node - "$TMP/incomplete-manual.json" "$TMP/incomplete-evidence.json" <<'NODE'
const fs = require('node:fs');
const [manifestPath, evidencePath] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
manifest.manualAcceptance.rows[0].evidence = evidencePath;
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
expect_failure "manual evidence missing required check" "$VERIFY" --manifest "$TMP/incomplete-manual.json"

write_manifest "$TMP/tampered-fixture.json" pass pass "$TMP/evidence.txt"
cp "$TMP/manual.json" "$TMP/tampered-evidence.json"
node - "$TMP/tampered-fixture.json" "$TMP/tampered-evidence.json" <<'NODE'
const fs = require('node:fs');
const [manifestPath, evidencePath] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const receipt = JSON.parse(fs.readFileSync(evidencePath, 'utf8'));
receipt.fixtureSha256['tests/fixtures/html/semantic.html'] = 'f'.repeat(64);
fs.writeFileSync(evidencePath, `${JSON.stringify(receipt, null, 2)}\n`);
manifest.manualAcceptance.rows[0].evidence = evidencePath;
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
expect_failure "manual evidence with wrong fixture hash" "$VERIFY" --manifest "$TMP/tampered-fixture.json"

printf 'verify-parity regressions: PASS\n'
