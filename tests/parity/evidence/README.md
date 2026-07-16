# Manual parity evidence

This directory contains the seven passing receipts produced by the real macOS print acceptance
matrix. `scripts/verify-parity.sh` rejects a row marked
`pass` unless its JSON receipt identifies the exact application and bundle ID, macOS version, arm64
architecture, application version, fixture SHA-256 values, non-empty Markdown SHA-256/size, warning
codes and every required acceptance check.

Use one file per manifest row, for example `safari.json`:

```json
{
  "schemaVersion": 1,
  "rowId": "safari",
  "result": "pass",
  "application": "Safari",
  "bundleId": "com.apple.Safari",
  "applicationVersion": "observed version",
  "generatedAt": "RFC 3339 timestamp",
  "environment": {"platform": "macOS", "osVersion": "observed version", "architecture": "arm64"},
  "fixtureSha256": {"tests/parity/fixtures/web-acceptance.html": "64 lowercase hex characters"},
  "output": {"markdownSha256": "64 lowercase hex characters", "bytes": 1},
  "warnings": [],
  "checks": {
    "reading_order": "pass",
    "headings": "pass",
    "lists": "pass",
    "tables": "pass",
    "links": "pass",
    "images": "pass",
    "warnings": "pass",
    "success_output": "pass",
    "opened_in_mdviewer": "pass",
    "cancellation_no_output": "pass",
    "cleanup_empty_store": "pass"
  }
}
```

When a fixture intentionally has no instance of a semantic feature, use
`{"result":"not_applicable","reason":"fixture-specific reason"}` instead of `"pass"`. A failure or
an unobserved check remains pending; it is never encoded as not applicable.
