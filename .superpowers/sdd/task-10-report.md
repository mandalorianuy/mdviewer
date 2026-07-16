# Task 10 Implementation Report

## Scope and result

Implemented Task 10 only: bounded local conversion for ZIP, EPUB, DOCX, PPTX,
XLSX, PNG, and JPEG. The local v1 registry is explicit and contains PDF,
HTML, CSV, JSON, XML, ZIP, EPUB, DOCX, PPTX, XLSX, PNG, and JPEG. No UI, CLI,
job system, release flow, OCR engine, or network client was added.

All converters return the shared `mdconvert_core::Document` model. The checked-in
goldens are produced by the shared GFM emitter; format converters do not build
Markdown strings.

## Implemented behavior

- ZIP: validates the central directory and matching local headers entirely in
  memory, authenticates a complete gap-free sequence of physical local records
  and data descriptors, caps archive budgets by the request input budget, sorts
  normalized names deterministically, converts the first local CSV/JSON/XML/HTML
  entry, warns when additional convertible entries are skipped, and otherwise
  emits a deterministic inventory.
- EPUB: requires the literal first stored entry to be the exact EPUB mimetype,
  using physical local-record order, resolves namespace-authenticated
  `container.xml`, OPF manifest/spine/navigation, and local assets inside the
  package, validates XHTML as strict XML, and passes direct bounded asset
  references to the HTML byte converter without data-URL round trips. Image
  sources accept only manifest-authenticated normalized package paths; authored
  schemes and the private internal reference namespace fail closed. HTML
  element and attribute names are ASCII-case-canonicalized before sanitizer
  policy checks, so authored uppercase aliases cannot bypass private-reference
  rejection.
- DOCX: reads relationships, metadata, styles with `basedOn` inheritance,
  bounded iterative `basedOn` inheritance, numbering, headings, every
  successive nested-list run, paragraphs, bold/italic runs, tables, safe local
  links, and authenticated local image parts.
- PPTX: follows presentation relationship order, preserves slide titles/body,
  shape-tree order across text/tables/images/groups, authenticated local images,
  safe run links, unambiguous notes, and page count. Skipped-link warnings are
  page-scoped and deduplicated.
- XLSX: follows workbook relationship order, supports shared and inline
  strings, percentage display styles, and emits formulas as code alongside
  cached displayed values. A1 references, row/cell ordering, dimensions, sparse
  allocation, and external-data parts/relationships fail closed. Formula
  contents are never evaluated; external-workbook and DDE references in cell
  formulas or defined names are rejected before emission. The quote-aware
  formula tokenizer handles doubled apostrophes in quoted sheet names, ignores
  string literals, recognizes DDE pipes only in executable formula text, and
  rejects the network/external-data functions WEBSERVICE, RTD, IMAGE, and
  STOCKHISTORY despite casing or whitespace variations. Pure local FILTERXML
  remains supported. The complete materialized table rectangle is checked
  against the cell budget.
- PNG/JPEG: preserves original local image assets, dimensions, and bounded
  document-semantic metadata without raster rendering. Width, height, and pixel
  count are checked before allocation. PNG validates chunk order, CRCs, bounded
  image-data expansion, exact stream boundaries, scanline filter bytes, and
  color-type-specific PLTE/tRNS rules, including the required PLTE-before-tRNS
  order whenever both chunks are present. Adam7 returns typed `UnsupportedInput`;
  successful PNG metadata reports
  `png.interlace_profile=non_interlaced_only`. JPEG validates frame components,
  scan selectors/parameters, stuffing, DRI structure, per-scan restart-marker
  sequence, legal DRI redefinition between scans, and Ri=0 restart disabling,
  plus multiscan state, entropy, and terminal EOI. Technical-only metadata does
  not suppress `OcrDeferred`; Task 10 never invokes or requires OCR.

The HTML crate gained a bounded `convert_bytes` entry point so EPUB and ZIP can
reuse its semantic conversion without writing package members to disk.

## Security properties

The archive reader performs no filesystem extraction. It rejects preambles,
unexplained gaps/overlaps/trailing local-record bytes, invalid descriptors, NUL names,
absolute/drive/UNC paths, `..`, duplicate normalized paths, symlinks and other
special entries, encryption, unsupported flags/compression, AES/ZIP64,
central/local metadata disagreement, CRC mismatch, truncation, corrupt EOCD,
deflate trailing bytes, and nested archives. Configurable non-zero limits cover
entry count, per-entry compressed/uncompressed bytes, total expanded bytes, and
expansion ratio, and are never looser than the request input budget. Backslashes
are normalized before rejecting UNC, drive, absolute, scheme, and escaping
package paths. Expanded XML names authenticate EPUB/OOXML envelopes,
relationships, content types, main parts, and supported content. EPUB asset
limits are cumulative across spine items and deduplicate only repeated part
references. OOXML images require declared media types and matching binary
structural validation; local v1 accepts PNG/JPEG only, while GIF/BMP/WebP/SVG
and external images fail closed. Relationship `TargetMode` is a closed enum,
and missing/unsafe hyperlinks preserve text with deduplicated scoped warnings.
External links are never fetched. XLSX external links, connections, queries,
external formulas, and any external relationship fail closed. OOXML Strict
namespace/relationship families are detected from parsed, authenticated content
types, root relationships, and main-part expanded names and return typed
`unsupported_input`; raw package substrings and ordinary document text do not
produce false Strict classifications. Successful OOXML metadata reports
`ooxml_profile=transitional_only`.

The production dependency graph passes:

```text
! cargo tree --workspace | rg -i 'reqwest|hyper|curl'
```

No advisory scanner is installed: `cargo audit --version` and
`cargo deny --version` both report `no such command`. No network installation
was attempted, so advisory-database validation is unavailable rather than
claimed as passed.

## TDD evidence

Initial Task 10 RED:

```text
cargo test -p mdconvert-formats --test container_formats --test image_without_ocr
```

Failed to compile because the Task 10 converter APIs did not exist. Subsequent
focused RED cases demonstrated rejection gaps for local-header size mismatch,
EPUB cumulative assets, PPTX run links, XLSX external-link parts, EPUB first
entry ordering, DOCX inherited styles, deflate trailing bytes, PNG iTXt, and
generic ZIP selection/warnings. Each focused case failed before its production
change and passed afterward.

Review-fix RED cases then reproduced the reported gaps before each production
change: request-budget bypass; missing/corrupt ZIP descriptors and physical
record gaps; physical EPUB mimetype ordering; UNC package targets; forged XML
namespaces/attributes and missing OPC envelopes; bogus, external, and SVG OOXML
images; dropped DOCX list runs and deep style chains; equal-but-distinct EPUB
assets; invalid/out-of-order/sparse XLSX references, dimensions, connections,
and external relationships; oversized image dimensions, malformed JPEG scans,
and technical-only metadata; PPTX shape order, warning scope/deduplication, and
ambiguous notes. Each is now covered by a passing regression.

Second-review RED cases reproduced the materialized XLSX rectangle allocation,
external-workbook/DDE formulas and defined names, EPUB authored schemes/private
asset aliases/deduplicated manifest paths, header-only and invalid-stream
embedded images, malformed JPEG SOF/SOS/stuffing/multiscan state, unknown
relationship modes, backslash UNC and missing hyperlinks, partial OOXML Strict
acceptance, and a signatureless ZIP descriptor whose CRC equals the optional
signature magic. Each test failed for the reported behavior before its focused
production change and is now GREEN.

Third-review RED cases reproduced six remaining gaps before their production
changes: (1) uppercase EPUB element/attribute aliases bypassing private asset
reference checks; (2) invalid PNG scanline filters and incomplete PLTE/tRNS
validation; (3) Adam7 being classified as corrupt instead of the explicit
non-interlaced-only `UnsupportedInput` profile; (4) JPEG restart markers being
accepted without a valid DRI segment or in the wrong sequence; (5) raw OOXML
Strict substring scanning both missing escaped namespace URIs and rejecting
ordinary Transitional document/binary content; and (6) XLSX external formulas
bypassing checks through quoted workbook names with escaped apostrophes or
function casing/whitespace. All six focused regressions are now GREEN, including
an embedded OOXML PNG with an invalid filter byte.

Fourth-review RED cases reproduced three P1 gaps: (1) a truecolor PNG with
tRNS before an optional PLTE was accepted both standalone and as an embedded
OOXML image; (2) valid JPEG DRI Ri=0 and legal interval redefinition/disabling
between scans were rejected, while restart state needed to follow the interval
active for each scan; and (3) XLSX treated pipes inside quoted local sheet names
as DDE, rejected local FILTERXML, and accepted network-backed IMAGE. The focused
tests failed for those exact behaviors before production changes and are now
GREEN. Valid PLTE-before-tRNS, DRI=0 without RST, DRI changes between scans,
FILTERXML, string literals, and `'A|B'!A1` remain accepted; RST after disable,
true DDE, and case/whitespace variants of WEBSERVICE, RTD, and IMAGE fail closed.

Final focused ZIP GREEN:

```text
cargo test -p mdconvert-formats --test container_formats \
  zip_converts_the_first_supported_entry_in_normalized_order_without_extraction -- --exact
```

Result: 1 passed, 0 failed.

## Final validation

- `cargo test -p mdconvert-formats`: 80 passed, 0 failed (41 container, 12 image,
  27 structured-format tests).
- `cargo test -p mdconvert-core`: 63 passed, 0 failed.
- `cargo test -p mdconvert-html`: 18 passed, 0 failed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Network dependency gate above: passed.
- `PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" ./scripts/verify-workspace.sh`:
  passed, including 222 Rust tests, doc tests, TypeScript checking, 3 frontend
  tests, and the production frontend build.
- `./scripts/verify-legacy-swift.sh`: 90 executed, 0 failures.
- `git diff --check`: passed.

## Fixtures and goldens

Portable authored fixtures:

| File | SHA-256 |
| --- | --- |
| `bounded.zip` | `788aae8ff3a1f48853876d132d8b2b4d24314291ce315783deb1e2831b420cb1` |
| `semantic.docx` | `419dc2b660126552c6db381c9967549dcc53112bf2444b4991ceaf1cc306ec31` |
| `ordered.pptx` | `34117f8143fcae8f8090bdbe1e867240225170cffe29ad137db4a2dd05016cb7` |
| `displayed.xlsx` | `f6fcf0b389320a861993e9207905886429349b6bf349553e2d67a0dad5a5aa77` |
| `spine.epub` | `97e4b332f5a7013f5b4ff3bc97501a932c6624ff1bc952f3e5414111343e097e` |
| `metadata.png` | `71dc4d61468db9840d56618e134e80e5d0ab3754668b8ec19e479f29df516fab` |
| `metadata.jpg` | `14f7a5e76f7cb0210ae140c4fbcd2ad77a9605bdc281a0557fd00dbb6024ad31` |

Shared-emitter goldens:

| File | SHA-256 |
| --- | --- |
| `bounded.zip.md` | `9df4545ab5f134847669f0d4ed7aec437ba0db33b1362bba1e3f5bc7a26abfa2` |
| `semantic.docx.md` | `eaebffd780016a7c8fc91e602a9be9f9c703eef1d4fe5c1db5de3e88de4b2951` |
| `ordered.pptx.md` | `b990fc7e3e00736c7bcc2c1ba72d2a08af4917903546be7bca8d3abdf559c748` |
| `displayed.xlsx.md` | `c577f85f2daffa4d5edaac5985b167fb3c7d15ba0a9dbc02aae99e00f67e7139` |
| `spine.epub.md` | `b0c0f04e6295bc08190c5b1f9bcbea2c8a69efc013fb6c319b361fcf7fb3b0e4` |
| `metadata.png.md` | `65e169ea70f1c364f4e8f3aee620b98ba0d91384d537890b6f84fed583fcd281` |
| `metadata.jpg.md` | `034e39df56dae237605dc09cc387b1be4d8aedcf76108f85bab974822a17e2f6` |

## Known limitations and residual risk

- Generic ZIP v1 converts only a top-level CSV, JSON, XML, HTML, or HTM entry;
  unsupported-only archives remain inventories and nested archives fail closed.
- EPUB XHTML is intentionally stricter than permissive browser HTML because it
  is validated as XML before semantic conversion.
- OOXML support targets the Task 10 semantic subset, not full rendering parity
  with Office applications. Local v1 is explicitly Transitional-only; ISO
  Strict packages return typed `unsupported_input`. Unsupported layout/style
  detail is not invented.
- XLSX cached values are trusted only as package data and formulas are not
  recalculated.
- Image v1 supports structurally validated PNG/JPEG plus bounded embedded
  metadata only; GIF/BMP/WebP/SVG are rejected and there is no pixel OCR or
  general EXIF rendering pipeline. PNG validation is explicitly
  non-interlaced-only; Adam7 returns typed `unsupported_input` and is not
  claimed as validated.
- The unavailable advisory scanner is the only validation gap recorded above.
