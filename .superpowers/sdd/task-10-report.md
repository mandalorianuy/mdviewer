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
  references to the HTML byte converter without data-URL round trips.
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
  contents are never evaluated.
- PNG/JPEG: preserves original local image assets, dimensions, and bounded
  document-semantic metadata without decoding pixels. Width, height, and pixel
  count are checked before allocation; JPEG requires a frame, scan, entropy,
  and terminal EOI. Technical-only metadata does not suppress `OcrDeferred`;
  Task 10 never invokes or requires OCR.

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
signatures; SVG and external images fail closed. External links are never
fetched. XLSX external links, connections, queries, and any external
relationship fail closed.

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

Final focused ZIP GREEN:

```text
cargo test -p mdconvert-formats --test container_formats \
  zip_converts_the_first_supported_entry_in_normalized_order_without_extraction -- --exact
```

Result: 1 passed, 0 failed.

## Final validation

- `cargo test -p mdconvert-formats`: 67 passed, 0 failed (32 container, 8 image,
  27 structured-format tests).
- `cargo test -p mdconvert-core`: 63 passed, 0 failed.
- `cargo test -p mdconvert-html`: 18 passed, 0 failed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Network dependency gate above: passed.
- `PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" ./scripts/verify-workspace.sh`:
  passed, including 209 Rust tests, doc tests, TypeScript checking, 3 frontend
  tests, and the production frontend build.
- `./scripts/verify-legacy-swift.sh`: 90 executed, 0 failures.
- `git diff --check`: passed.

## Fixtures and goldens

Portable authored fixtures:

| File | SHA-256 |
| --- | --- |
| `bounded.zip` | `788aae8ff3a1f48853876d132d8b2b4d24314291ce315783deb1e2831b420cb1` |
| `semantic.docx` | `9efd96a953f356ee200b5cefdfe6c2c0f78003817697579594b2ef9fab25810d` |
| `ordered.pptx` | `34117f8143fcae8f8090bdbe1e867240225170cffe29ad137db4a2dd05016cb7` |
| `displayed.xlsx` | `f6fcf0b389320a861993e9207905886429349b6bf349553e2d67a0dad5a5aa77` |
| `spine.epub` | `a81fd9e379a9673cb00ce5ab18f6f6554b369010e0c1e1aa7f1cacaa861cea09` |
| `metadata.png` | `f04e818d291f20fbe6dec8ec1ad52452a7c810a9caa944d35478b57ba40bfbc9` |
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
  with Office applications. Unsupported layout/style detail is not invented.
- XLSX cached values are trusted only as package data and formulas are not
  recalculated.
- Image v1 supports PNG/JPEG dimensions and bounded embedded metadata only;
  there is no pixel OCR or general EXIF rendering pipeline.
- The unavailable advisory scanner is the only validation gap recorded above.
