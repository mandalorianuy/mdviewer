# MDViewer v1 parity and Swift retirement gate

## Current status

**STRICT PARITY PASSED — SWIFT RETIRED.** All seven real application rows pass with checked-in
receipts and the strict parity command reports no pending behavior or manual row. The annotated
`swift-baseline-final` tag resolves to pre-removal commit
`8b6d73427693251e5cee7e786dc500013f862815` on both GitHub and OneDev. The active tree no longer
contains `legacy/macos-swift`, and both workspace and strict parity gates pass after removal. This is
parity evidence, not a notarized release claim.

The machine authority is `tests/parity/manifest.json`. The strict command is
`./scripts/verify-parity.sh`; it must reject any required behavior or available application row that
is still pending. `./scripts/verify-parity.sh --automated` is the pre-matrix gate: it executes every
declared automated suite, validates test selectors and approved exclusions, but reports pending
manual rows without authorizing retirement.

## Export parity implemented

The frozen Swift inventory includes HTML and PDF export, and the approved design requires existing
export needed for parity. The Tauri desktop now exports a standalone, sanitized HTML document from
the rendered React preview through an opaque HTML-only Save selector and transactional writer. It
ships a fail-closed no-network CSP, strips active content plus URL- and CSS-bearing attributes, and
inlines allowlisted local preview images under explicit size/count budgets. Image bodies are read as
bounded streams so an undeclared oversized body is cancelled before it can be fully materialized;
remote image URLs are never fetched. It also exposes universal PDF export through the native WebView
print dialog with a preview-only, light-safe print stylesheet and temporary document title. Print
state and title are restored even when the native dialog fails. Both behaviors have checked-in
automated evidence; Save and Save As Markdown remain separate behaviors.

## Automated behavior inventory

The manifest maps the approved viewer, editor, Save, Save As, HTML/PDF export, close, find, sanitized
GFM preview, appearance preference, direct conversion, progress/cancellation, warnings, result
opening, every
local v1 converter, macOS alias lifecycle, cold/warm print delivery, native Save As, transactional
GFM/assets and cleanup behavior to a checked-in test selector and executable suite.

The only approved exclusions are:

- OCR and reliable scanned-PDF conversion: v1.1. Digital PDFs without extractable text return the
  stable OCR-required diagnosis.
- YouTube and other network-backed conversion sources: post-v1 network-import design. They are not
  part of the local v1 registry.

## Exact macOS acceptance matrix

The acceptance host is Apple Silicon and all seven planned applications were exercised through the
real Print → PDF Service → native Save As flow. Every row includes a successful conversion review,
a cancellation review and a checked-in receipt under `tests/parity/evidence/`.

| Row | Exact application identity | Local non-sensitive fixtures | Success review | Cancellation/cleanup review | Receipt |
|---|---|---|---|---|---|
| Safari | Safari, `com.apple.Safari` | `tests/parity/fixtures/web-acceptance.html` | **pass** — order, H1, ordered list, table, safe link and extracted image | **pass** — no output; store empty | `tests/parity/evidence/safari.json` |
| Mail | Mail, `com.apple.mail` | `tests/parity/fixtures/mail-acceptance.eml` | **pass** — order, H1/H2, list, table, safe link and CID image | **pass** — no output; store empty | `tests/parity/evidence/mail.json` |
| TextEdit | TextEdit, `com.apple.TextEdit` | `tests/parity/fixtures/textedit-acceptance.rtf` | **pass** — order, H2, list, table and link; image explicitly N/A because TextEdit did not render the RTF pict block | **pass** — no output; store empty | `tests/parity/evidence/textedit.json` |
| Preview | Preview, `com.apple.Preview` | `tests/parity/fixtures/preview-acceptance.pdf` | **pass** — order, H1/H2, list, table, visible URL link and extracted image | **pass** — no output; store empty | `tests/parity/evidence/preview.json` |
| Word | Microsoft Word, `com.microsoft.Word` | `tests/parity/fixtures/word-acceptance.docx` | **pass** — order, H1, ordered list, table and visible URL; image fixture-specifically N/A | **pass** — no output; store empty | `tests/parity/evidence/word.json` |
| Excel | Microsoft Excel, `com.microsoft.Excel` | `tests/parity/fixtures/excel-acceptance.xlsx` | **pass** — displayed formula values and GFM table; absent document semantics explicitly N/A | **pass** — no output; store empty | `tests/parity/evidence/excel.json` |
| PowerPoint | Microsoft PowerPoint, `com.microsoft.Powerpoint` | `tests/parity/fixtures/powerpoint-acceptance.pptx` | **pass** — slide order/title/body and reviewed warning; absent semantics explicitly N/A | **pass** — no output; store empty | `tests/parity/evidence/powerpoint.json` |

For each row:

1. Hash every declared fixture and record the exact application version, macOS version and arm64
   architecture.
2. Open the fixture in the exact bundle, choose Print → PDF → Guardar como Markdown con MDViewer,
   accept Save As to a unique temporary destination and wait for MDViewer to open it.
3. Inspect structure in the Markdown and preview, record stable warning codes, then hash and measure
   the non-empty Markdown. Do not record sensitive content or machine-local output paths.
4. Repeat from the same application, choose a different unused output name and cancel Save As (or
   cancel conversion when exercising that branch). Prove that output is absent and the private
   print-job store is empty.
5. Attach one JSON receipt matching `tests/parity/evidence/README.md`, set only that manifest row to
   `pass`, and rerun the strict gate. Any reproducible defect becomes a local fixture and regression
   before it can be fixed.

## Retirement sequence

The fail-closed retirement sequence completed in order:

1. Swift passed its frozen 90-test baseline while still present.
2. Every real application receipt and strict `verify-parity.sh` passed.
3. The annotated baseline tag was pushed and verified on both remotes.
4. The active Swift tree and its obsolete verifier were removed together, then workspace and strict
   parity gates passed again.
5. Signing, notarization, Gatekeeper and release publication remain later credentialed gates; they
   are not implied by parity retirement.
